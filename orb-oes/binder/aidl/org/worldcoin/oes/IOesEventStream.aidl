package org.worldcoin.oes;

/** Receives Orb Event Stream (OES) events pushed by other on-device processes. */
interface IOesEventStream {
    /** mode: 0 = Normal, 1 = Sticky, 2 = CacheOnly (mirrors oes::Mode). */
    void pushEvent(String name, String payloadJson, int mode);
}
