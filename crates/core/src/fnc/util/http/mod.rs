use crate::cnf::SURREALDB_USER_AGENT;
use crate::ctx::Context;
use crate::err::Error;
use crate::expr::{Bytes, Object, Strand, Value};
use crate::syn;
use anyhow::{Context as _, Result, bail};
use reqwest::header::CONTENT_TYPE;
#[cfg(not(target_family = "wasm"))]
use reqwest::redirect::Attempt;
#[cfg(not(target_family = "wasm"))]
use reqwest::redirect::Policy;
use reqwest::{Client, Method, RequestBuilder, Response};
#[cfg(not(target_family = "wasm"))]
use tokio::runtime::Handle;
use url::Url;

pub(crate) fn uri_is_valid(uri: &str) -> bool {
	reqwest::Url::parse(uri).is_ok()
}

fn encode_body(req: RequestBuilder, body: Value) -> RequestBuilder {
	match body {
		Value::Bytes(v) => req.body(v.0),
		Value::Strand(v) => req.body(v.0),
		_ if body.is_some() => req.json(&body.into_json()),
		_ => req,
	}
}

async fn decode_response(res: Response) -> Result<Value> {
	match res.error_for_status() {
		Ok(res) => match res.headers().get(CONTENT_TYPE) {
			Some(mime) => match mime.to_str() {
				Ok(v) if v.starts_with("application/json") => {
					let txt = res.text().await.map_err(Error::from)?;
					let val = syn::json(&txt)
						.context("Failed to parse JSON response")
						.map_err(|e| Error::Http(e.to_string()))?;
					Ok(val.into())
				}
				Ok(v) if v.starts_with("application/octet-stream") => {
					let bytes = res.bytes().await.map_err(Error::from)?;
					Ok(Value::Bytes(Bytes(bytes.into())))
				}
				Ok(v) if v.starts_with("text") => {
					let txt = res.text().await.map_err(Error::from)?;
					let val = txt.into();
					Ok(val)
				}
				_ => Ok(Value::None),
			},
			_ => Ok(Value::None),
		},
		Err(err) => match err.status() {
			Some(s) => bail!(Error::Http(format!(
				"{} {}",
				s.as_u16(),
				s.canonical_reason().unwrap_or_default(),
			))),
			None => bail!(Error::Http(err.to_string())),
		},
	}
}

async fn request(
	ctx: &Context,
	method: Method,
	uri: Strand,
	body: Option<Value>,
	opts: impl Into<Object>,
) -> Result<Value> {
	// Check if the URI is valid and allowed
	let url = Url::parse(&uri).map_err(|_| Error::InvalidUrl(uri.to_string()))?;
	ctx.check_allowed_net(&url).await?;
	// Set a default client with no timeout

	let builder = Client::builder();

	#[cfg(not(target_family = "wasm"))]
	let builder = {
		let count = *crate::cnf::MAX_HTTP_REDIRECTS;
		let ctx_clone = ctx.clone();
		let policy = Policy::custom(move |attempt: Attempt| {
			let check = tokio::task::block_in_place(|| {
				Handle::current().block_on(ctx_clone.check_allowed_net(attempt.url()))
			});
			if let Err(e) = check {
				return attempt.error(e);
			}
			if attempt.previous().len() >= count {
				return attempt.stop();
			}
			attempt.follow()
		});
		let b = builder.redirect(policy);
		b.dns_resolver(std::sync::Arc::new(
			crate::fnc::http::resolver::FilteringResolver::from_capabilities(
				ctx.get_capabilities(),
			),
		))
	};

	let cli = builder.build()?;

	let is_head = matches!(method, Method::HEAD);
	// Start a new HEAD request
	let mut req = cli.request(method.clone(), url);
	// Add the User-Agent header
	if cfg!(not(target_family = "wasm")) {
		req = req.header(reqwest::header::USER_AGENT, &*SURREALDB_USER_AGENT);
	}
	// Add specified header values
	for (k, v) in opts.into().iter() {
		req = req.header(k.as_str(), v.to_raw_string());
	}

	if let Some(b) = body {
		// Submit the request body
		req = encode_body(req, b);
	}

	// Send the request and wait
	let res = match ctx.timeout() {
		#[cfg(not(target_family = "wasm"))]
		Some(d) => req.timeout(d).send().await.map_err(Error::from)?,
		_ => req.send().await.map_err(Error::from)?,
	};

	if is_head {
		// Check the response status
		match res.error_for_status() {
			Ok(_) => Ok(Value::None),
			Err(err) => match err.status() {
				Some(s) => bail!(Error::Http(format!(
					"{} {}",
					s.as_u16(),
					s.canonical_reason().unwrap_or_default(),
				))),
				None => bail!(Error::Http(err.to_string())),
			},
		}
	} else {
		// Receive the response as a value
		decode_response(res).await
	}
}

pub async fn head(ctx: &Context, uri: Strand, opts: impl Into<Object>) -> Result<Value> {
	request(ctx, Method::HEAD, uri, None, opts).await
}

pub async fn get(ctx: &Context, uri: Strand, opts: impl Into<Object>) -> Result<Value> {
	request(ctx, Method::GET, uri, None, opts).await
}

pub async fn put(
	ctx: &Context,
	uri: Strand,
	body: Value,
	opts: impl Into<Object>,
) -> Result<Value> {
	request(ctx, Method::PUT, uri, Some(body), opts).await
}

pub async fn post(
	ctx: &Context,
	uri: Strand,
	body: Value,
	opts: impl Into<Object>,
) -> Result<Value> {
	request(ctx, Method::POST, uri, Some(body), opts).await
}

pub async fn patch(
	ctx: &Context,
	uri: Strand,
	body: Value,
	opts: impl Into<Object>,
) -> Result<Value> {
	request(ctx, Method::PATCH, uri, Some(body), opts).await
}

pub async fn delete(ctx: &Context, uri: Strand, opts: impl Into<Object>) -> Result<Value> {
	request(ctx, Method::DELETE, uri, None, opts).await
}
