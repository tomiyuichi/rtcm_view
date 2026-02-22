#include "rtklib_wrapper.h"
#include <stdlib.h>
#include <string.h>

/* Satellite system and PRN from satellite number */
static void sat2info(int sat, uint8_t *sys, uint8_t *prn) {
    int p;
    int s = satsys(sat, &p);
    *sys = (uint8_t)s;
    *prn = (uint8_t)p;
}

rtcm_t *rtklib_alloc_rtcm(void) {
    rtcm_t *rtcm = (rtcm_t *)calloc(1, sizeof(rtcm_t));
    if (!rtcm) return NULL;
    if (!init_rtcm(rtcm)) {
        free(rtcm);
        return NULL;
    }
    rtcm->outtype = 1; /* enable msgtype string output in decode_rtcm3 */
    return rtcm;
}

void rtklib_free_rtcm(rtcm_t *rtcm) {
    if (!rtcm) return;
    free_rtcm(rtcm);
    free(rtcm);
}

void rtklib_input_rtcm3(rtcm_t *rtcm, uint8_t data, rtcm_decode_result_t *out) {
    memset(out, 0, sizeof(*out));

    /* Clear msgtype before call so we can detect if decode_rtcm3 ran */
    rtcm->msgtype[0] = '\0';

    out->ret = input_rtcm3(rtcm, data);

    /* If msgtype was written, a complete frame was decoded (even if ret=0 due to sync) */
    if (rtcm->msgtype[0] != '\0') {
        const char *p = rtcm->msgtype;
        while (*p && (*p < '0' || *p > '9')) p++;
        while (*p >= '0' && *p <= '9') {
            out->msg_type = out->msg_type * 10 + (*p - '0');
            p++;
        }

        strncpy(out->msg_type_str, rtcm->msgtype, sizeof(out->msg_type_str) - 1);
        out->msg_type_str[sizeof(out->msg_type_str) - 1] = '\0';

        /* If ret==0 but a message was decoded (sync=1 MSM), signal it as ret=-10 */
        if (out->ret == 0) {
            out->ret = -10; /* decoded but synced (more messages in epoch) */
        }
    }
}

int rtklib_get_obs_count(const rtcm_t *rtcm) {
    if (!rtcm) return 0;
    return rtcm->obs.n;
}

int rtklib_get_obs(const rtcm_t *rtcm, int index, rtcm_obs_t *out) {
    if (!rtcm || !out || index < 0 || index >= rtcm->obs.n) return 0;
    if (!rtcm->obs.data) return 0;

    const obsd_t *d = &rtcm->obs.data[index];
    memset(out, 0, sizeof(*out));

    out->time_sec = (double)d->time.time;
    out->frac_sec = d->time.sec;
    out->sat = d->sat;
    sat2info(d->sat, &out->sys, &out->prn);

    int nf = NFREQ + NEXOBS;
    if (nf > 3) nf = 3;

    for (int i = 0; i < nf; i++) {
        out->code[i] = d->code[i];
        out->P[i] = d->P[i];
        out->L[i] = d->L[i];
        out->D[i] = d->D[i];
        out->SNR[i] = d->SNR[i];
        out->LLI[i] = d->LLI[i];
    }

    return 1;
}

int rtklib_get_sta(const rtcm_t *rtcm, rtcm_sta_t *out) {
    if (!rtcm || !out) return 0;

    memset(out, 0, sizeof(*out));
    out->staid = rtcm->staid;
    out->pos[0] = rtcm->sta.pos[0];
    out->pos[1] = rtcm->sta.pos[1];
    out->pos[2] = rtcm->sta.pos[2];
    out->hgt = rtcm->sta.hgt;

    strncpy(out->antdes, rtcm->sta.antdes, sizeof(out->antdes) - 1);
    strncpy(out->antsno, rtcm->sta.antsno, sizeof(out->antsno) - 1);
    strncpy(out->rectype, rtcm->sta.rectype, sizeof(out->rectype) - 1);
    strncpy(out->recver, rtcm->sta.recver, sizeof(out->recver) - 1);

    return 1;
}

int rtklib_get_eph_summary(const rtcm_t *rtcm, rtcm_eph_summary_t *out) {
    if (!rtcm || !out || rtcm->ephsat <= 0) return 0;

    memset(out, 0, sizeof(*out));
    out->sat = (uint8_t)rtcm->ephsat;
    sat2info(rtcm->ephsat, &out->sys, &out->prn);

    int sat = rtcm->ephsat;

    /* Search GPS/GAL/BDS/QZS/IRN ephemeris */
    const eph_t *eph = rtcm->nav.eph;
    if (eph) {
        for (int i = 0; i < rtcm->nav.n; i++) {
            if (eph[i].sat == sat) {
                out->iode = eph[i].iode;
                out->svh = eph[i].svh;
                out->toe_sec = (double)eph[i].toe.time + eph[i].toe.sec;
                return 1;
            }
        }
    }

    /* Search GLONASS ephemeris */
    const geph_t *geph = rtcm->nav.geph;
    if (geph) {
        for (int i = 0; i < rtcm->nav.ng; i++) {
            if (geph[i].sat == sat) {
                out->iode = geph[i].iode;
                out->svh = geph[i].svh;
                out->toe_sec = (double)geph[i].toe.time + geph[i].toe.sec;
                return 1;
            }
        }
    }

    return 1;
}

int rtklib_get_msg_counts(const rtcm_t *rtcm, int *types, uint32_t *counts, int max_entries) {
    if (!rtcm || !types || !counts) return 0;

    int n = 0;
    /* RTCM3: index 1-299 → types 1001-1299, index 300-329 → types 4070-4099 */
    for (int i = 1; i < 400 && n < max_entries; i++) {
        if (rtcm->nmsg3[i] > 0) {
            if (i < 300) {
                types[n] = 1000 + i;
            } else {
                types[n] = 4070 + (i - 300);
            }
            counts[n] = rtcm->nmsg3[i];
            n++;
        }
    }
    /* Index 0 = "other" */
    if (rtcm->nmsg3[0] > 0 && n < max_entries) {
        types[n] = 0;
        counts[n] = rtcm->nmsg3[0];
        n++;
    }

    return n;
}
