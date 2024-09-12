pub const TRACE_ID_HEADER: &str = "X-Trace-Id";

#[derive(Copy, Clone, Default)]
pub struct TraceConfig {
    pub log_req_headers: bool,
    pub log_resp_headers: bool,
    pub log_req_body_size: u64,
    pub log_resp_body_size: u64,
    pub only_on_error: bool,
    pub always_log_headers: bool,
}

macro_rules! def_tracer {
    ($vis:vis $ident:ident) => {
        #[derive(Clone)]
        $vis struct $ident(TraceConfig);

        impl $ident {
            $vis fn trace_only() -> Self {
                Self(TraceConfig::default())
            }
            $vis fn log_headers() -> Self {
                Self(TraceConfig {
                    log_req_headers: true,
                    log_resp_headers: true,
                    ..Default::default()
                })
            }
            $vis fn log_body(max_size: u64) -> Self {
                Self(TraceConfig {
                    log_req_body_size: max_size,
                    log_resp_body_size: max_size,
                    ..Default::default()
                })
            }
            $vis fn log_all(body_max_size: u64) -> Self {
                Self(TraceConfig {
                    log_req_headers: true,
                    log_resp_headers: true,
                    log_req_body_size: body_max_size,
                    log_resp_body_size: body_max_size,
                    ..Default::default()
                })
            }
            $vis fn log_req_headers(self) -> Self {
                Self(TraceConfig { log_req_headers: true, ..self.0 })
            }
            $vis fn log_resp_headers(self) -> Self {
                Self(TraceConfig { log_resp_headers: true, ..self.0 })
            }
            $vis fn log_req_body(self, max_size: u64) -> Self {
                Self(TraceConfig { log_req_body_size: max_size, ..self.0 })
            }
            $vis fn log_resp_body(self, max_size: u64) -> Self {
                Self(TraceConfig { log_resp_body_size: max_size, ..self.0 })
            }
            $vis fn only_on_error(self, always_log_headers: bool) -> Self {
                Self(TraceConfig { only_on_error: true, always_log_headers, ..self.0 })
            }
        }
    };
}
pub(crate) use def_tracer;

macro_rules! def_format_headers {
    ($ident:ident) => {
        fn format_headers(headers: &$ident) -> String {
            let mut buf = String::new();
            for (k, v) in headers {
                buf.push_str(k.as_str());
                buf.push_str(": ");
                buf.push_str(&String::from_utf8_lossy(v.as_bytes()).to_owned());
                buf.push_str("\n");
            }
            buf
        }
    };
}
pub(crate) use def_format_headers;