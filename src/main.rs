use eframe::egui;
use std::io::Read;
use std::net::TcpStream;
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread;
use std::time::Duration;

fn main() {
    let options = eframe::NativeOptions::default();
    let _ = eframe::run_native(
        "RTCM View",
        options,
        Box::new(|_cc| Box::new(RtcmViewApp::default())),
    );
}

#[derive(Debug, Clone, PartialEq)]
enum ConnectionStatus {
    Disconnected,
    Connecting,
    Connected,
    Error(String),
}

enum StreamEvent {
    Connected,
    Data(Vec<u8>),
    Error(String),
    Disconnected,
}

struct RtcmViewApp {
    host: String,
    port: String,
    connection_status: ConnectionStatus,
    rx: Option<Receiver<StreamEvent>>,
    stop_tx: Option<Sender<()>>,
    received_data: Vec<u8>,
    log_messages: Vec<String>,
}

impl Default for RtcmViewApp {
    fn default() -> Self {
        Self {
            host: "127.0.0.1".to_string(),
            port: "2101".to_string(),
            connection_status: ConnectionStatus::Disconnected,
            rx: None,
            stop_tx: None,
            received_data: Vec::new(),
            log_messages: Vec::new(),
        }
    }
}

impl RtcmViewApp {
    fn connect(&mut self, ctx: &egui::Context) {
        let addr = format!("{}:{}", self.host, self.port);
        self.connection_status = ConnectionStatus::Connecting;
        self.received_data.clear();
        self.log_messages.clear();
        self.log_messages
            .push(format!("Connecting to {}...", addr));

        let (event_tx, event_rx) = mpsc::channel::<StreamEvent>();
        let (stop_tx, stop_rx) = mpsc::channel::<()>();

        self.rx = Some(event_rx);
        self.stop_tx = Some(stop_tx);

        let ctx = ctx.clone();
        thread::spawn(move || {
            let stream = match TcpStream::connect(&addr) {
                Ok(s) => s,
                Err(e) => {
                    let _ = event_tx.send(StreamEvent::Error(format!(
                        "Connection failed: {}",
                        e
                    )));
                    ctx.request_repaint();
                    return;
                }
            };

            // Set read timeout so we can check the stop signal periodically
            let _ = stream.set_read_timeout(Some(Duration::from_millis(100)));
            let _ = event_tx.send(StreamEvent::Connected);
            ctx.request_repaint();

            let mut stream = stream;
            let mut buf = [0u8; 4096];

            loop {
                // Check for stop signal
                if stop_rx.try_recv().is_ok() {
                    let _ = event_tx.send(StreamEvent::Disconnected);
                    ctx.request_repaint();
                    return;
                }

                match stream.read(&mut buf) {
                    Ok(0) => {
                        // Connection closed by remote
                        let _ = event_tx.send(StreamEvent::Disconnected);
                        ctx.request_repaint();
                        return;
                    }
                    Ok(n) => {
                        let _ = event_tx.send(StreamEvent::Data(buf[..n].to_vec()));
                        ctx.request_repaint();
                    }
                    Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock
                        || e.kind() == std::io::ErrorKind::TimedOut =>
                    {
                        // Read timeout - continue loop to check stop signal
                        continue;
                    }
                    Err(e) => {
                        let _ = event_tx.send(StreamEvent::Error(format!(
                            "Read error: {}",
                            e
                        )));
                        ctx.request_repaint();
                        return;
                    }
                }
            }
        });
    }

    fn disconnect(&mut self) {
        if let Some(stop_tx) = self.stop_tx.take() {
            let _ = stop_tx.send(());
        }
        self.rx = None;
        self.connection_status = ConnectionStatus::Disconnected;
        self.log_messages.push("Disconnected.".to_string());
    }

    fn poll_events(&mut self) {
        let Some(rx) = &self.rx else { return };

        while let Ok(event) = rx.try_recv() {
            match event {
                StreamEvent::Connected => {
                    self.connection_status = ConnectionStatus::Connected;
                    self.log_messages.push("Connected.".to_string());
                }
                StreamEvent::Data(data) => {
                    self.log_messages
                        .push(format!("Received {} bytes", data.len()));
                    self.received_data.extend_from_slice(&data);
                }
                StreamEvent::Error(msg) => {
                    self.connection_status =
                        ConnectionStatus::Error(msg.clone());
                    self.log_messages.push(format!("Error: {}", msg));
                    self.stop_tx = None;
                    self.rx = None;
                    return;
                }
                StreamEvent::Disconnected => {
                    self.connection_status = ConnectionStatus::Disconnected;
                    self.log_messages
                        .push("Remote disconnected.".to_string());
                    self.stop_tx = None;
                    self.rx = None;
                    return;
                }
            }
        }
    }
}

fn format_hex_dump(data: &[u8]) -> String {
    let mut result = String::new();
    for (i, chunk) in data.chunks(16).enumerate() {
        let offset = i * 16;
        result.push_str(&format!("{:08x}  ", offset));

        for (j, byte) in chunk.iter().enumerate() {
            result.push_str(&format!("{:02x} ", byte));
            if j == 7 {
                result.push(' ');
            }
        }

        // Padding for incomplete lines
        let padding = 16 - chunk.len();
        for j in 0..padding {
            result.push_str("   ");
            if chunk.len() + j == 7 {
                result.push(' ');
            }
        }

        result.push_str(" |");
        for byte in chunk {
            if byte.is_ascii_graphic() || *byte == b' ' {
                result.push(*byte as char);
            } else {
                result.push('.');
            }
        }
        result.push_str("|\n");
    }
    result
}

impl eframe::App for RtcmViewApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.poll_events();

        egui::TopBottomPanel::top("connection_panel").show(ctx, |ui| {
            ui.heading("RTCM Stream Viewer");
            ui.separator();

            ui.horizontal(|ui| {
                ui.label("Host:");
                let host_edit = egui::TextEdit::singleline(&mut self.host)
                    .desired_width(150.0);
                ui.add(host_edit);

                ui.label("Port:");
                let port_edit = egui::TextEdit::singleline(&mut self.port)
                    .desired_width(60.0);
                ui.add(port_edit);

                let is_connected = matches!(
                    self.connection_status,
                    ConnectionStatus::Connected | ConnectionStatus::Connecting
                );

                if is_connected {
                    if ui.button("Disconnect").clicked() {
                        self.disconnect();
                    }
                } else if ui.button("Connect").clicked() {
                    self.connect(ctx);
                }
            });

            ui.horizontal(|ui| {
                ui.label("Status: ");
                match &self.connection_status {
                    ConnectionStatus::Disconnected => {
                        ui.colored_label(egui::Color32::GRAY, "Disconnected");
                    }
                    ConnectionStatus::Connecting => {
                        ui.colored_label(egui::Color32::YELLOW, "Connecting...");
                    }
                    ConnectionStatus::Connected => {
                        ui.colored_label(egui::Color32::GREEN, "Connected");
                    }
                    ConnectionStatus::Error(msg) => {
                        ui.colored_label(
                            egui::Color32::RED,
                            format!("Error: {}", msg),
                        );
                    }
                }

                ui.with_layout(
                    egui::Layout::right_to_left(egui::Align::Center),
                    |ui| {
                        ui.label(format!(
                            "Received: {} bytes",
                            self.received_data.len()
                        ));
                    },
                );
            });
        });

        egui::CentralPanel::default().show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.heading("Hex Dump");
                if ui.button("Clear").clicked() {
                    self.received_data.clear();
                    self.log_messages.clear();
                }
            });
            ui.separator();

            egui::ScrollArea::vertical()
                .auto_shrink([false; 2])
                .stick_to_bottom(true)
                .show(ui, |ui| {
                    if self.received_data.is_empty() {
                        ui.colored_label(
                            egui::Color32::GRAY,
                            "No data received yet.",
                        );
                    } else {
                        let hex = format_hex_dump(&self.received_data);
                        ui.label(
                            egui::RichText::new(&hex)
                                .monospace()
                                .size(12.0),
                        );
                    }
                });
        });
    }
}
