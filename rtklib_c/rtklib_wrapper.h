#ifndef RTKLIB_WRAPPER_H
#define RTKLIB_WRAPPER_H

#include "rtklib.h"

/* Flat observation record for FFI */
typedef struct {
    double time_sec;    /* GPS time (seconds since epoch) */
    double frac_sec;    /* fractional seconds */
    uint8_t sat;        /* satellite number */
    uint8_t sys;        /* satellite system (SYS_GPS=0x01, SYS_GLO=0x04, etc.) */
    uint8_t prn;        /* PRN number */
    uint8_t code[3];    /* signal codes per frequency */
    double P[3];        /* pseudorange (m) per frequency */
    double L[3];        /* carrier phase (cycles) per frequency */
    float D[3];         /* Doppler (Hz) per frequency */
    float SNR[3];       /* signal strength (dBHz) per frequency */
    uint8_t LLI[3];     /* loss of lock indicator per frequency */
} rtcm_obs_t;

/* Flat station info for FFI */
typedef struct {
    int staid;          /* station ID */
    double pos[3];      /* ECEF position (m) */
    double hgt;         /* antenna height (m) */
    char antdes[64];    /* antenna descriptor */
    char antsno[64];    /* antenna serial number */
    char rectype[64];   /* receiver type */
    char recver[64];    /* receiver firmware version */
} rtcm_sta_t;

/* Flat ephemeris summary for FFI */
typedef struct {
    uint8_t sat;        /* satellite number */
    uint8_t sys;        /* satellite system */
    uint8_t prn;        /* PRN number */
    int iode;           /* issue of data */
    int svh;            /* SV health */
    double toe_sec;     /* time of ephemeris (s) */
} rtcm_eph_summary_t;

/* Return value info from input_rtcm3 */
typedef struct {
    int ret;            /* return code: 0=none, 1=obs, 2=eph, 5=sta, 10=ssr */
    int msg_type;       /* RTCM3 message type number */
    char msg_type_str[256]; /* message type description string */
} rtcm_decode_result_t;

/* Allocate and initialize an rtcm_t struct (sets outtype=1 for msgtype) */
rtcm_t *rtklib_alloc_rtcm(void);

/* Free an rtcm_t struct */
void rtklib_free_rtcm(rtcm_t *rtcm);

/* Feed one byte into the RTCM3 decoder. Writes result to *out. */
void rtklib_input_rtcm3(rtcm_t *rtcm, uint8_t data, rtcm_decode_result_t *out);

/* Get the number of observations in the last decoded message */
int rtklib_get_obs_count(const rtcm_t *rtcm);

/* Get a single observation record (index 0..n-1) */
int rtklib_get_obs(const rtcm_t *rtcm, int index, rtcm_obs_t *out);

/* Get station information */
int rtklib_get_sta(const rtcm_t *rtcm, rtcm_sta_t *out);

/* Get ephemeris summary for the last decoded ephemeris */
int rtklib_get_eph_summary(const rtcm_t *rtcm, rtcm_eph_summary_t *out);

/* Get message type counts: returns total number of different types received */
int rtklib_get_msg_counts(const rtcm_t *rtcm, int *types, uint32_t *counts, int max_entries);

#endif /* RTKLIB_WRAPPER_H */
