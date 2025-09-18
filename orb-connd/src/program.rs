use orb_backend_status_dbus::BackendStatusT;

struct Program {
    mm: Box<dyn ModemManager>,
    nm: Box<dyn NetworkManager>,
    bs: Box<dyn BackendStatusT>,
    sd: Box<dyn StatsdClient>,
    nsc: NetStatsCollector,
}

trait ModemManager {}
trait NetworkManager {}
trait StatsdClient {}

//  telemetry:
//      - modem telemetry collector (worker)
//      - modem statsd reporter (worker)
//      - modem backend status reporter (worker)
//      - connectivity config backend status reporter (worker)
//
//  conn cellular: 
//      - cfg and establish on startup (worker)
//
//  wifi:
//      - import old config on startup
//
//  dbus (service):
//      - add wifi profile
//      - remove wifi profile
//      - apply netconfig qr
//      - apply wifi qr (with restrictions)
//      - toggle smart switching

