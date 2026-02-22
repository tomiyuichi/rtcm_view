#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod rtklib_ffi;

const GIT_DESCRIBE: &str = env!("GIT_DESCRIBE");
const GIT_HASH: &str = env!("GIT_HASH");

use eframe::egui;
use rtklib_ffi::{RtcmDecoder, RtcmEvent};
use std::io::Read;
use std::net::TcpStream;
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread;
use std::time::Duration;

fn config_path() -> std::path::PathBuf {
    std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|p| p.to_path_buf()))
        .unwrap_or_default()
        .join("rtcm_view.conf")
}

fn load_config() -> (String, String) {
    let content = std::fs::read_to_string(config_path()).unwrap_or_default();
    let mut host = "127.0.0.1".to_string();
    let mut port = "2101".to_string();
    for line in content.lines() {
        if let Some(v) = line.strip_prefix("host=") { host = v.to_string(); }
        else if let Some(v) = line.strip_prefix("port=") { port = v.to_string(); }
    }
    (host, port)
}

fn save_config(host: &str, port: &str) {
    let _ = std::fs::write(config_path(), format!("host={}\nport={}\n", host, port));
}

fn main() {
    let options = eframe::NativeOptions::default();
    let _ = eframe::run_native(
        "RTCM View",
        options,
        Box::new(|_cc| Box::new(RtcmViewApp::default())),
    );
}

#[derive(Debug, Clone, PartialEq, Default)]
enum Screen {
    #[default]
    Main,
    Rtcm3Inspector,
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
    Rtcm(RtcmEvent),
    Error(String),
    Disconnected,
}

/// Summary of a decoded RTCM3 message for the inspector log
#[derive(Debug, Clone)]
struct RtcmMessageLog {
    msg_type: i32,
    msg_desc: String,
    detail: String,
}

struct RtcmViewApp {
    host: String,
    port: String,
    connection_status: ConnectionStatus,
    rx: Option<Receiver<StreamEvent>>,
    stop_tx: Option<Sender<()>>,
    received_bytes: usize,
    received_data: Vec<u8>,
    log_messages: Vec<String>,
    current_screen: Screen,
    // RTCM3 Inspector state
    rtcm_messages: Vec<RtcmMessageLog>,
    msg_type_counts: std::collections::BTreeMap<i32, u32>,
}

impl Drop for RtcmViewApp {
    fn drop(&mut self) {
        save_config(&self.host, &self.port);
    }
}

impl Default for RtcmViewApp {
    fn default() -> Self {
        let (host, port) = load_config();
        Self {
            host,
            port,
            connection_status: ConnectionStatus::Disconnected,
            rx: None,
            stop_tx: None,
            received_bytes: 0,
            received_data: Vec::new(),
            log_messages: Vec::new(),
            current_screen: Screen::default(),
            rtcm_messages: Vec::new(),
            msg_type_counts: std::collections::BTreeMap::new(),
        }
    }
}

impl RtcmViewApp {
    fn connect(&mut self, ctx: &egui::Context) {
        let addr = format!("{}:{}", self.host, self.port);
        self.connection_status = ConnectionStatus::Connecting;
        self.received_bytes = 0;
        self.received_data.clear();
        self.log_messages.clear();
        self.rtcm_messages.clear();
        self.msg_type_counts.clear();
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

            let _ =
                stream.set_read_timeout(Some(Duration::from_millis(100)));
            let _ = event_tx.send(StreamEvent::Connected);
            ctx.request_repaint();

            let mut stream = stream;
            let mut buf = [0u8; 4096];

            // Create RTCM3 decoder in this thread
            let mut decoder = match RtcmDecoder::new() {
                Some(d) => d,
                None => {
                    let _ = event_tx.send(StreamEvent::Error(
                        "Failed to initialize RTCM decoder".to_string(),
                    ));
                    ctx.request_repaint();
                    return;
                }
            };

            loop {
                if stop_rx.try_recv().is_ok() {
                    let _ = event_tx.send(StreamEvent::Disconnected);
                    ctx.request_repaint();
                    return;
                }

                match stream.read(&mut buf) {
                    Ok(0) => {
                        let _ = event_tx.send(StreamEvent::Disconnected);
                        ctx.request_repaint();
                        return;
                    }
                    Ok(n) => {
                        let data = buf[..n].to_vec();
                        let _ = event_tx.send(StreamEvent::Data(data.clone()));

                        // Feed each byte to the RTCM3 decoder
                        for &byte in &data {
                            if let Some(event) = decoder.input(byte) {
                                let _ =
                                    event_tx.send(StreamEvent::Rtcm(event));
                            }
                        }
                        ctx.request_repaint();
                    }
                    Err(ref e)
                        if e.kind() == std::io::ErrorKind::WouldBlock
                            || e.kind()
                                == std::io::ErrorKind::TimedOut =>
                    {
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
                    self.received_bytes += data.len();
                    // Keep only last 16KB of raw data for hex dump
                    self.received_data.extend_from_slice(&data);
                    const MAX_RAW: usize = 16 * 1024;
                    if self.received_data.len() > MAX_RAW {
                        let excess = self.received_data.len() - MAX_RAW;
                        self.received_data.drain(..excess);
                    }
                }
                StreamEvent::Rtcm(rtcm_event) => {
                    let log = rtcm_event_to_log(&rtcm_event);
                    *self
                        .msg_type_counts
                        .entry(log.msg_type)
                        .or_insert(0) += 1;
                    self.rtcm_messages.push(log);
                    // Keep last 500 messages
                    if self.rtcm_messages.len() > 500 {
                        self.rtcm_messages
                            .drain(..self.rtcm_messages.len() - 500);
                    }
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

fn rtcm_event_to_log(event: &RtcmEvent) -> RtcmMessageLog {
    match event {
        RtcmEvent::Observation {
            msg_type,
            msg_desc,
            observations,
        } => {
            let sats: Vec<String> = observations
                .iter()
                .map(|o| {
                    format!(
                        "{}{:02}",
                        rtklib_ffi::sys_name(o.sys),
                        o.prn
                    )
                })
                .collect();
            RtcmMessageLog {
                msg_type: *msg_type,
                msg_desc: msg_desc.clone(),
                detail: format!("{} sats: {}", observations.len(), sats.join(" ")),
            }
        }
        RtcmEvent::Ephemeris {
            msg_type,
            msg_desc,
            summary,
        } => RtcmMessageLog {
            msg_type: *msg_type,
            msg_desc: msg_desc.clone(),
            detail: format!(
                "{}{:02} IODE={}",
                rtklib_ffi::sys_name(summary.sys),
                summary.prn,
                summary.iode
            ),
        },
        RtcmEvent::Station {
            msg_type,
            msg_desc,
            staid,
            pos,
            antdes,
            ..
        } => RtcmMessageLog {
            msg_type: *msg_type,
            msg_desc: msg_desc.clone(),
            detail: format!(
                "ID={} pos=({:.1},{:.1},{:.1}) ant={}",
                staid, pos[0], pos[1], pos[2], antdes
            ),
        },
        RtcmEvent::Synced {
            msg_type,
            msg_desc,
        } => RtcmMessageLog {
            msg_type: *msg_type,
            msg_desc: msg_desc.clone(),
            detail: "(sync=1, more follow)".to_string(),
        },
        RtcmEvent::Ssr {
            msg_type,
            msg_desc,
        } => RtcmMessageLog {
            msg_type: *msg_type,
            msg_desc: msg_desc.clone(),
            detail: "SSR correction".to_string(),
        },
        RtcmEvent::Other {
            msg_type,
            msg_desc,
            ret,
        } => RtcmMessageLog {
            msg_type: *msg_type,
            msg_desc: msg_desc.clone(),
            detail: format!("ret={}", ret),
        },
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

// -- UI screens --

impl RtcmViewApp {
    fn show_main_screen(&mut self, ctx: &egui::Context) {
        egui::TopBottomPanel::top("connection_panel").show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.heading("RTCM Stream Viewer");
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.label(
                        egui::RichText::new(format!("{} ({})", GIT_DESCRIBE, GIT_HASH))
                            .small()
                            .color(egui::Color32::GRAY),
                    );
                });
            });
            ui.separator();

            ui.horizontal(|ui| {
                ui.label("Host:");
                ui.add(
                    egui::TextEdit::singleline(&mut self.host)
                        .desired_width(150.0),
                );

                ui.label("Port:");
                ui.add(
                    egui::TextEdit::singleline(&mut self.port)
                        .desired_width(60.0),
                );

                let is_connected = matches!(
                    self.connection_status,
                    ConnectionStatus::Connected
                        | ConnectionStatus::Connecting
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
                        ui.colored_label(
                            egui::Color32::GRAY,
                            "Disconnected",
                        );
                    }
                    ConnectionStatus::Connecting => {
                        ui.colored_label(
                            egui::Color32::YELLOW,
                            "Connecting...",
                        );
                    }
                    ConnectionStatus::Connected => {
                        ui.colored_label(
                            egui::Color32::GREEN,
                            "Connected",
                        );
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
                            "Received: {} bytes | RTCM msgs: {}",
                            self.received_bytes,
                            self.rtcm_messages.len()
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
                    self.received_bytes = 0;
                    self.log_messages.clear();
                }
            });
            ui.separator();

            let is_connected =
                self.connection_status == ConnectionStatus::Connected;
            ui.add_enabled_ui(is_connected, |ui| {
                if ui.button("RTCM3_Inspector").clicked() {
                    self.current_screen = Screen::Rtcm3Inspector;
                }
            });
            if !is_connected {
                ui.label(
                    egui::RichText::new(
                        "Connect to a stream to enable inspector.",
                    )
                    .small()
                    .color(egui::Color32::GRAY),
                );
            }
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

    fn show_rtcm3_inspector(&mut self, ctx: &egui::Context) {
        egui::TopBottomPanel::top("inspector_top").show(ctx, |ui| {
            ui.horizontal(|ui| {
                if ui.button("< Back").clicked() {
                    self.current_screen = Screen::Main;
                }
                ui.heading("RTCM3 Inspector");

                ui.with_layout(
                    egui::Layout::right_to_left(egui::Align::Center),
                    |ui| {
                        ui.label(format!(
                            "Messages: {}",
                            self.rtcm_messages.len()
                        ));
                    },
                );
            });
        });

        egui::SidePanel::left("msg_type_panel")
            .default_width(200.0)
            .show(ctx, |ui| {
                ui.heading("Message Types");
                ui.separator();

                if self.msg_type_counts.is_empty() {
                    ui.colored_label(
                        egui::Color32::GRAY,
                        "No messages yet.",
                    );
                } else {
                    egui::ScrollArea::vertical().show(ui, |ui| {
                        egui::Grid::new("msg_type_grid")
                            .striped(true)
                            .show(ui, |ui| {
                                ui.strong("Type");
                                ui.strong("Count");
                                ui.end_row();

                                for (&mt, &count) in &self.msg_type_counts {
                                    ui.label(format!("{}", mt));
                                    ui.label(format!("{}", count));
                                    ui.end_row();
                                }
                            });
                    });
                }
            });

        egui::CentralPanel::default().show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.heading("Message Log");
                if ui.button("Clear").clicked() {
                    self.rtcm_messages.clear();
                    self.msg_type_counts.clear();
                }
            });
            ui.separator();

            egui::ScrollArea::vertical()
                .auto_shrink([false; 2])
                .stick_to_bottom(true)
                .show(ui, |ui| {
                    if self.rtcm_messages.is_empty() {
                        ui.colored_label(
                            egui::Color32::GRAY,
                            "Waiting for RTCM3 messages...",
                        );
                    } else {
                        egui::Grid::new("rtcm_msg_grid")
                            .striped(true)
                            .min_col_width(60.0)
                            .show(ui, |ui| {
                                ui.strong("Type");
                                ui.strong("Description");
                                ui.strong("Detail");
                                ui.end_row();

                                for msg in &self.rtcm_messages {
                                    ui.label(format!("{}", msg.msg_type));
                                    ui.label(
                                        egui::RichText::new(&msg.msg_desc)
                                            .monospace()
                                            .size(11.0),
                                    );
                                    ui.label(
                                        egui::RichText::new(&msg.detail)
                                            .monospace()
                                            .size(11.0),
                                    );
                                    ui.end_row();
                                }
                            });
                    }
                });
        });
    }
}

impl eframe::App for RtcmViewApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.poll_events();

        match self.current_screen {
            Screen::Main => self.show_main_screen(ctx),
            Screen::Rtcm3Inspector => self.show_rtcm3_inspector(ctx),
        }
    }
}
