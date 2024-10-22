# Guideline for Supervised Process Development

Examples of SuPr (**Su**pervised **Pr**ocess) development:
- update-agent
- orb-core
- fan-controller
- ...

## Expectations

Through signal_hook or otherwise, we expect components to adhere to UNIX signal best practices, specifically around shutdown signals.

### Shutdown Flow

The supervisor _decides_ it must shutdown. The supervisor iterates over the list of supervised processes, reads their corresponding PID file, and issues a [SIGTERM](https://man7.org/linux/man-pages/man7/signal.7.html) to give the application **SOME DEFINED SECONDS** to shutdown. After that time has elapsed, the supervisor re-reads the SuPr PID files and sends a [SIGKILL](https://man7.org/linux/man-pages/man7/signal.7.html).
