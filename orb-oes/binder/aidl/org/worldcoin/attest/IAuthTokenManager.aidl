package org.worldcoin.attest;

/** Exposes the Orb's backend auth token to Android apps over Binder. */
interface IAuthTokenManager {
    /** Current backend auth token. Throws if none has been fetched yet. */
    String getToken();

    /** Request an out-of-band token refresh. */
    void forceTokenRefresh();
}
