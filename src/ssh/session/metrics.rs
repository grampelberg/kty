use lazy_static::lazy_static;
use prometheus::{
    histogram_opts, opts, register_histogram, register_int_counter, register_int_counter_vec,
    register_int_gauge, Histogram, IntCounter, IntCounterVec, IntGauge,
};
use prometheus_static_metric::make_static_metric;

make_static_metric! {
    pub struct MethodVec: IntCounter {
        "method" => {
            publickey,
            interactive,
        }
    }
    pub struct ResultVec: IntCounter {
        "method" => {
            publickey,
            interactive,
        },
        "result" => {
            accept,
            partial,
            reject,
        }
    }
    pub struct CodeVec: IntCounter {
        "result" => {
            valid,
            invalid,
        }
    }
    pub struct RequestVec: IntCounter {
        "method" => {
            pty,
            sftp,
            window_resize,
            tcpip_forward,
        }
    }
    pub struct ChannelVec: IntCounter {
        "method" => {
            open_session,
            close,
            eof,
            direct_tcpip,
        }
    }
}

lazy_static! {
    pub static ref TOTAL_BYTES: IntCounter =
        register_int_counter!("bytes_received_total", "Total number of bytes received").unwrap();
    pub static ref TOTAL_SESSIONS: IntCounter =
        register_int_counter!("session_total", "Total number of sessions").unwrap();
    pub static ref ACTIVE_SESSIONS: IntGauge =
        register_int_gauge!("active_sessions", "Number of active sessions").unwrap();
    pub static ref SESSION_DURATION: Histogram = register_histogram!(histogram_opts!(
        "session_duration_minutes",
        "Session duration",
        vec!(0.5, 1.0, 2.0, 5.0, 10.0, 30.0, 60.0),
    ))
    .unwrap();
    pub static ref UNEXPECTED_STATE: IntCounterVec = register_int_counter_vec!(
        opts!(
            "unexpected_state_total",
            "Number of times an unexpected state was encountered",
        ),
        &["expected", "actual"],
    )
    .unwrap();
}

lazy_static! {
    static ref AUTH_ATTEMPTS_VEC: IntCounterVec = register_int_counter_vec!(
        opts!(
            "auth_attempts_total",
            "Number of authentication attempts. Note that this can seem inflated because \
             `publickey` will always be attempted first and `keyboard` will happen at least twice \
             for every success."
        ),
        &["method"]
    )
    .unwrap();
    pub static ref AUTH_ATTEMPTS: MethodVec = MethodVec::from(&AUTH_ATTEMPTS_VEC);
    static ref AUTH_RESULTS_VEC: IntCounterVec = register_int_counter_vec!(
        opts!(
            "auth_results_total",
            "Counter for the results of authentication attempts. Note that this can seem inflated \
             because `publickey` is always attempted first and provides a rejection before moving \
             onto other methods",
        ),
        &["method", "result"],
    )
    .unwrap();
    pub static ref AUTH_RESULTS: ResultVec = ResultVec::from(&AUTH_RESULTS_VEC);
    pub static ref AUTH_SUCEEDED: IntCounterVec = register_int_counter_vec!(
        opts!(
            "auth_succeeded_total",
            "Number of sessions that reached `auth_succeeded`."
        ),
        &["method"],
    )
    .unwrap();
    pub static ref CODE_GENERATED: IntCounter =
        register_int_counter!("code_generated_total", "Number of device codes generated").unwrap();
    static ref CODE_CHECKED_VEC: IntCounterVec = register_int_counter_vec!(
        opts!(
            "code_checked_total",
            "Number of times device codes have been checked",
        ),
        &["result"],
    )
    .unwrap();
    pub static ref CODE_CHECKED: CodeVec = CodeVec::from(&CODE_CHECKED_VEC);
}

lazy_static! {
    static ref REQUESTS_VEC: IntCounterVec =
        register_int_counter_vec!(opts!("requests_total", "Number of requests",), &["method"])
            .unwrap();
    pub static ref REQUESTS: RequestVec = RequestVec::from(&REQUESTS_VEC);
}

lazy_static! {
    static ref CHANNELS_VEC: IntCounterVec = register_int_counter_vec!(
        opts!("channels_total", "Number of channel actions",),
        &["method"]
    )
    .unwrap();
    pub static ref CHANNELS: ChannelVec = ChannelVec::from(&CHANNELS_VEC);
}
