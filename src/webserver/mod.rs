use crate::proxywireprotocol::{ApiResponse, CounterSnapshot, CounterType};
use crate::{
    exporter::{Exporter, ExporterFactory},
    proxy_common::{concat_slices, hostname},
};

use colored::Colorize;
use rouille::{Request, Response};
use serde::Deserialize;
use static_files::Resource;
use std::collections::HashMap;
use std::path::Path;
use std::sync::{Arc, Mutex};

include!(concat!(env!("OUT_DIR"), "/generated.rs"));

/*************
 * WEBSERVER *
 *************/

struct ClientPivot {
    url: String,
    refcount: u32,
    child: Vec<String>,
}

impl ClientPivot {
    fn new(url: String) -> ClientPivot {
        ClientPivot {
            url,
            refcount: 0,
            child: Vec::new(),
        }
    }

    fn mapto(&mut self, child_url: String) {
        self.refcount += 1;
        self.child.push(child_url);
    }

    fn is_partial(&self) -> bool {
        if (self.refcount < 2) && (1 < self.refcount) {
            return true;
        }

        false
    }

    fn is_free(&self) -> bool {
        if self.refcount < 2 {
            return true;
        }

        false
    }
}

pub(crate) struct Web {
    port: u32,
    factory: Arc<ExporterFactory>,
    static_files: HashMap<String, Resource>,
    known_client: Mutex<Vec<ClientPivot>>,
}

enum WebResponse {
    #[allow(unused)]
    Html(String),
    StaticHtml(String, &'static str, Vec<u8>),
    Text(String),
    BadReq(String),
    Success(String),
    #[allow(unused)]
    Redirect302(String),
    Native(Response),
    NoSuchDoc(),
}

impl WebResponse {
    fn serialize(self: WebResponse) -> Response {
        match self {
            WebResponse::Html(s) => Response::html(s),
            WebResponse::StaticHtml(name, mime, data) => {
                log::trace!("{} {} as {}", "STATIC".yellow(), name, mime);
                Response::from_data(mime, data)
            }
            WebResponse::Text(s) => Response::text(s),
            WebResponse::BadReq(operation) => {
                let r = ApiResponse {
                    operation,
                    success: false,
                };
                Response::json(&r).with_status_code(400)
            }
            WebResponse::Success(operation) => {
                let r = ApiResponse {
                    operation,
                    success: true,
                };
                Response::json(&r)
            }
            WebResponse::NoSuchDoc() => Response::empty_404(),
            WebResponse::Redirect302(url) => Response::redirect_302(url),
            WebResponse::Native(response) => response,
        }
    }
}

impl Web {
    pub(crate) fn new(port: u32, factory: Arc<ExporterFactory>) -> Web {
        let web = Web {
            port,
            factory,
            static_files: generate()
                .into_iter()
                .map(|(k, v)| (k.to_string(), v))
                .collect(),
            known_client: Mutex::new(Vec::new()),
        };
        /* Add myself in the URLs */
        web.known_client
            .lock()
            .unwrap()
            .push(ClientPivot::new(web.url()));
        web
    }

    pub(crate) fn url(&self) -> String {
        format!("{}:{}", hostname(), self.port)
    }

    fn default_doc() -> String {
        "".to_string()
    }

    fn parse_key_value(req: &Request) -> (Option<String>, Option<f64>, String, Option<String>) {
        let key: Option<String>;
        let svalue: Option<String>;
        let parsed_value: Option<f64>;
        let error: Option<String>;
        let doc: String; // Optionnal
        #[derive(Deserialize)]
        struct KeyValue {
            key: String,
            value: String,
            #[serde(default = "Web::default_doc")]
            doc: String,
        }

        match req.method() {
            "GET" => {
                key = req.get_param("key");
                svalue = req.get_param("value");
                match req.get_param("doc") {
                    Some(e) => {
                        doc = e;
                    }
                    None => {
                        doc = Web::default_doc();
                    }
                }
                error = None;
            }
            "POST" => {
                let kv: Result<KeyValue, rouille::input::json::JsonError> =
                    rouille::input::json_input(req);
                match kv {
                    Ok(v) => {
                        key = Some(v.key);
                        svalue = Some(v.value);
                        doc = v.doc;
                        error = None;
                    }
                    Err(e) => {
                        key = None;
                        svalue = None;
                        doc = Web::default_doc();
                        error = Some(e.to_string());
                    }
                }
            }
            _ => {
                key = None;
                svalue = None;
                doc = Web::default_doc();
                error = Some(format!("No such request type {}", req.method()));
            }
        }

        if let Some(val) = svalue {
            match val.parse::<f64>() {
                Ok(val) => {
                    parsed_value = Some(val);
                }
                Err(e) => {
                    return (key, None, doc, Some(e.to_string()));
                }
            }
        } else {
            parsed_value = None;
        }

        (key, parsed_value, doc, error)
    }

    fn handle_set(&self, req: &Request) -> WebResponse {
        let key: Option<String>;
        let error: Option<String>;
        let value: Option<f64>;
        let _doc: String;

        (key, value, _doc, error) = Web::parse_key_value(req);

        if error.is_some() {
            return WebResponse::BadReq(format!("Error parsing parameters: {}", error.unwrap()));
        }

        let key = key.unwrap();
        let value = value.unwrap();

        let snap = CounterSnapshot {
            name: key,
            doc: "".to_string(),
            ctype: CounterType::Counter { value },
        };

        match self.factory.get_main().set(snap) {
            Ok(_) => WebResponse::Success("set".to_string()),
            Err(e) => WebResponse::BadReq(e.to_string()),
        }
    }

    fn handle_accumulate(&self, req: &Request) -> WebResponse {
        let key: Option<String>;
        let error: Option<String>;
        let value: Option<f64>;
        let _doc: String;

        (key, value, _doc, error) = Web::parse_key_value(req);

        if error.is_some() {
            return WebResponse::BadReq(format!("Error parsing parameters: {}", error.unwrap()));
        }

        let key = key.unwrap();
        let value = value.unwrap();

        let snap = CounterSnapshot {
            name: key,
            doc: "".to_string(),
            ctype: CounterType::Counter { value },
        };

        match self.factory.get_main().accumulate(&snap) {
            Ok(_) => WebResponse::Success("inc".to_string()),
            Err(e) => WebResponse::BadReq(e.to_string()),
        }
    }

    fn handle_push(&self, req: &Request) -> WebResponse {
        let key: Option<String>;
        let error: Option<String>;
        let doc: String;

        (key, _, doc, error) = Web::parse_key_value(req);

        if error.is_some() {
            return WebResponse::BadReq(format!("Error parsing parameters: {}", error.unwrap()));
        }

        let key = key.unwrap();

        let snap = CounterSnapshot {
            name: key,
            doc,
            ctype: CounterType::newcounter(),
        };

        match self.factory.get_main().push(&snap) {
            Ok(_) => WebResponse::Success("push".to_string()),
            Err(e) => WebResponse::BadReq(e.to_string()),
        }
    }

    fn serialize_exporter(exporter: &Arc<Exporter>) -> WebResponse {
        match exporter.serialize() {
            Ok(v) => WebResponse::Text(v),
            Err(e) => WebResponse::BadReq(e.to_string()),
        }
    }

    fn handle_metrics(&self, req: &Request) -> WebResponse {
        if let Some(jobid) = req.get_param("job") {
            if let Some(exporter) = self.factory.resolve_by_id(&jobid) {
                Web::serialize_exporter(&exporter)
            } else {
                WebResponse::BadReq(format!("No such jobid {}", jobid))
            }
        } else {
            Web::serialize_exporter(&self.factory.get_main())
        }
    }

    fn handle_join(&self, req: &Request) -> WebResponse {
        let to = req.get_param("to");

        if to.is_none() {
            return WebResponse::BadReq("No to parameter passed".to_string());
        }

        let to = to.unwrap();

        if to.contains("http") {
            return WebResponse::BadReq(
                "To should not be an URL (with http://) but host:port".to_string(),
            );
        }

        let period: u64 = match req.get_param("period") {
            Some(e) => e.parse::<u64>().unwrap_or(5),
            None => 5,
        };

        if let Err(e) = ExporterFactory::add_scrape(self.factory.clone(), &to, period) {
            return WebResponse::BadReq(format!("Failed to add {} for scraping : {}", to, e));
        }

        WebResponse::Success(format!("Added {} for scraping", to))
    }

    fn handle_pivot(&self, req: &Request) -> WebResponse {
        let from = req.get_param("from");

        if from.is_none() {
            return WebResponse::BadReq("No from parameter passed".to_string());
        }

        let from: String = from.unwrap().clone();

        if from.contains("http") {
            return WebResponse::BadReq(
                "From should not be an URL (with http://) but host:port".to_string(),
            );
        }

        let mut clients = self.known_client.lock().unwrap();

        let mut target = clients.iter_mut().find(|v| v.is_partial());

        if target.is_none() {
            target = clients.iter_mut().find(|v| v.is_free());
        }

        let resp: WebResponse;

        if let Some(target) = target {
            target.mapto(from.to_string());

            log::info!(
                "Pivot response to {} is {} with ref {}",
                from,
                target.url,
                target.refcount
            );

            resp = WebResponse::Success(target.url.to_string());
        } else {
            resp = WebResponse::BadReq("Did not match any server".to_string());
        }

        clients.push(ClientPivot::new(from));

        resp
    }

    fn handle_topo(&self, _req: &Request) -> WebResponse {
        let mut resp: Vec<(String, String)> = Vec::new();

        for c in self.known_client.lock().unwrap().iter() {
            for t in &c.child {
                resp.push((c.url.clone(), t.clone()))
            }
        }

        WebResponse::Native(Response::json(&resp))
    }

    fn handle_job(&self, req: &Request) -> WebResponse {
        if let Some(jobid) = req.get_param("job") {
            match self.factory.profile_of(&jobid) {
                Ok(p) => WebResponse::Native(Response::json(&p)),
                Err(e) => WebResponse::BadReq(e.to_string()),
            }
        } else {
            let all = self.factory.profiles();
            WebResponse::Native(Response::json(&all))
        }
    }

    fn handle_joblist(&self, _req: &Request) -> WebResponse {
        let jobs = self.factory.list_jobs();

        match serde_json::to_vec(&jobs) {
            Ok(_v) => WebResponse::Native(Response::json(&jobs)),
            Err(e) => WebResponse::BadReq(e.to_string()),
        }
    }

    fn serve_static_file(&self, url: &str) -> WebResponse {
        /* remove slash before */
        assert!(url.starts_with('/'));

        let url = url[1..].to_string();

        if let Some(file) = self.static_files.get(&url) {
            /* Handle HTML templating on the fly */
            let data = if url.ends_with(".html") {
                let header = self.static_files.get("header.html.in").unwrap();
                let footer = self.static_files.get("footer.html.in").unwrap();
                concat_slices([header.data, file.data, footer.data])
            } else {
                file.data.to_vec()
            };

            WebResponse::StaticHtml(url, file.mime_type, data)
        } else {
            log::warn!("{} {}", "No such file".red(), url);
            WebResponse::NoSuchDoc()
        }
    }

    fn parse_url(surl: &str) -> (String, String) {
        let url = surl[1..].to_string();

        assert!(surl.starts_with('/'));

        let path = Path::new(&url);

        let segments: Vec<String> = path
            .iter()
            .map(|v| v.to_string_lossy().to_string())
            .collect();

        if segments.len() == 1 {
            /* Only a single entry /metric/ */
            return (segments[0].to_string(), "".to_string());
        } else if segments.is_empty() {
            /* Only / */
            return ("/".to_string(), "".to_string());
        }

        let prefix = segments
            .iter()
            .take(segments.len() - 1)
            .map(|v| v.to_string())
            .collect::<Vec<_>>()
            .join("/");
        let resource = segments.last().take().unwrap().to_string();

        (prefix, resource)
    }

    pub(crate) fn run_blocking(self) {
        let hostname = hostname();
        log::info!(
            "Proxy webserver listening on http://{}:{}",
            hostname,
            self.port
        );

        rouille::start_server(format!("0.0.0.0:{}", self.port), move |request| {
            let url = request.url();

            let (prefix, resource) = Web::parse_url(&url);

            log::trace!(
                "GET {} mapped to ({} , {})",
                request.raw_url(),
                prefix.red(),
                resource.yellow()
            );

            let resp: WebResponse = match prefix.as_str() {
                "/" => self.serve_static_file("/index.html"),
                "set" => self.handle_set(request),
                "accumulate" => self.handle_accumulate(request),
                "push" => self.handle_push(request),
                "metrics" => self.handle_metrics(request),
                "joblist" => self.handle_joblist(request),
                "job" => self.handle_job(request),
                "pivot" => self.handle_pivot(request),
                "topo" => self.handle_topo(request),
                "join" => self.handle_join(request),
                _ => self.serve_static_file(url.as_str()),
            };

            resp.serialize()
        });
    }
}
