use hyper::{Request, Response, Body, Server, StatusCode};
use hyper::http::request::Parts;
use futures::Future;
use hyper::service::service_fn;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::sync::{Arc, Mutex};
use super::errors::Error;

pub fn bad_request() -> Response<Body> {
	Response::builder().status(StatusCode::BAD_REQUEST).body(Body::empty()).unwrap()
}

pub fn invalid_method() -> Response<Body> {
	text_response(StatusCode::METHOD_NOT_ALLOWED, "Method not allowed")
}

pub fn bad_request_because(text: &'static str) -> Response<Body> {
	text_response(StatusCode::BAD_REQUEST, text)
}

pub fn json_response<T>(code: StatusCode, obj: &T) -> Response<Body> where T: serde::Serialize {
	let body = serde_json::to_string(obj).unwrap();
	Response::builder()
		.status(code)
		.header("Content-Type", "application/json; charset=utf-8")
		.body(Body::from(body))
		.unwrap()
}

pub fn text_response(code: StatusCode, text: &'static str) -> Response<Body> {
	Response::builder()
		.status(code)
		.header("Content-Type", "text/plain; charset=utf-8")
		.body(Body::from(text))
		.unwrap()
}

/// Wraps a regular async request in a wrapper that logs out errors and nicely responds to clients on errors
/// NOTE: The error type doesn't really matter as we never resolve to a error, just as long as it is sendable across threads, hyper won't complain
pub fn handle_request_guard<F, P, I>(
	req: Request<Body>, arg: I, f: F,
) -> impl Future<Item=Response<Body>, Error=std::io::Error>
	where P: Future<Item=Response<Body>, Error=Error>,
		  I: Clone,
		  F: Fn(Parts, Body, I) -> P {

	let (parts, body) = req.into_parts();

	// Mainly for being able to print out errors
	let method = parts.method.clone();
	let uri = parts.uri.clone();

	f(parts, body, arg).then(move |res| {
		match res {
			Ok(resp) => Ok(resp),
			Err(e) => {
				eprintln!("{} {}: {:?}", method, uri, e);
				Ok(Response::builder().status(500).body(Body::empty()).unwrap())
			}
		}
	})
}

// TODO: See https://docs.rs/hyper/0.12.19/hyper/server/struct.Server.html#example for graceful shutdowns
pub fn start_http_server<F, FS, FE, P: 'static, I: 'static>(
	port: u16, arg: &Arc<I>, f: &'static F, fstart: &FS, fend: &'static FE
)
	where P: Send + Future<Item=Response<Body>, Error=Error>,
		  I: Send + Sync,
		  F: Sync + (Fn(Parts, Body, Arc<I>) -> P),
		  FS: Fn(&Arc<I>),
		  FE: Sync + Fn(&Arc<I>)
{
	let addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), port);

	let (tx, rx) = futures::sync::oneshot::channel::<()>();

	let arg = arg.clone();
	let arg2 = arg.clone();
	let arg3 = arg.clone();
	let server = Server::bind(&addr)
        .serve(move || {
			let arg = arg.clone();
			service_fn(move |req: Request<Body>| {
				handle_request_guard(req, arg.clone(), f)				
			})
		})
		.with_graceful_shutdown(rx)
		.map_err(|e| eprintln!("HTTP Server Error: {}", e));

    println!("Listening on http://{}", addr);
	

	let tx_wrap = Arc::new(Mutex::new(Some(tx)));
	ctrlc::set_handler(move || {

		// Take the tx exactly once (all future ctrl-c's will get a None and return)
		let tx = match tx_wrap.lock().unwrap().take() {
			Some(tx) => tx,
			None => return
		};

		// Everything below here should only ever be called exactly once

		fend(&arg2);

		// Shutdown the server
		if let Err(e) = tx.send(()) {
			eprintln!("Error while shutting down: {:?}", e);
		}

    }).expect("Error setting Ctrl-C handler");

	fstart(&arg3);

	hyper::rt::run(server);

	println!("Shutdown!")
}

