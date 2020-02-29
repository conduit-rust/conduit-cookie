#![cfg_attr(test, deny(warnings))]
#![warn(rust_2018_idioms)]

extern crate base64;
extern crate conduit;
extern crate conduit_middleware;
#[cfg(test)]
extern crate conduit_test;
extern crate cookie;

use conduit::{header, RequestExt};
use conduit_middleware::{AfterResult, BeforeResult};
use cookie::{Cookie, CookieJar};

pub use session::{RequestSession, SessionMiddleware};

mod session;

#[derive(Default)]
pub struct Middleware {}

impl Middleware {
    pub fn new() -> Self {
        Default::default()
    }
}

fn parse_pair(key_value: &str) -> Option<(String, String)> {
    key_value.find('=').map(|i| {
        (
            key_value[..i].trim().into(),
            key_value[(i + 1)..].trim().into(),
        )
    })
}

impl conduit_middleware::Middleware for Middleware {
    fn before(&self, req: &mut dyn RequestExt) -> BeforeResult {
        let jar = {
            let headers = req.headers();
            let mut jar = CookieJar::new();
            for cookie in headers.get_all(header::COOKIE).iter() {
                if let Ok(cookie) = cookie.to_str() {
                    for cookie in cookie.split(';') {
                        if let Some((key, value)) = parse_pair(cookie) {
                            jar.add_original(Cookie::new(key, value));
                        }
                    }
                }
            }
            jar
        };
        req.mut_extensions().insert(jar);
        Ok(())
    }

    fn after(&self, req: &mut dyn RequestExt, res: AfterResult) -> AfterResult {
        use std::convert::TryInto;

        let mut res = res?;

        for delta in req.cookies().delta() {
            if let Ok(value) = delta.to_string().try_into() {
                res.headers_mut().append(header::SET_COOKIE, value);
            }
        }

        Ok(res)
    }
}

pub trait RequestCookies {
    fn cookies(&self) -> &CookieJar;
    fn cookies_mut(&mut self) -> &mut CookieJar;
}

impl<T: RequestExt + ?Sized> RequestCookies for T {
    fn cookies(&self) -> &CookieJar {
        self.extensions()
            .find::<CookieJar>()
            .expect("Missing cookie jar")
    }

    fn cookies_mut(&mut self) -> &mut CookieJar {
        self.mut_extensions()
            .find_mut::<CookieJar>()
            .expect("Missing cookie jar")
    }
}

#[cfg(test)]
mod tests {
    use conduit::{header, Body, Handler, HttpResult, Method, RequestExt, Response};
    use conduit_middleware::MiddlewareBuilder;
    use conduit_test::MockRequest;
    use cookie::Cookie;

    use super::{Middleware, RequestCookies};

    #[test]
    fn request_headers() {
        let mut req = MockRequest::new(Method::POST, "/articles");
        req.header(header::COOKIE, "foo=bar");

        let mut app = MiddlewareBuilder::new(test);
        app.add(Middleware::new());
        assert!(app.call(&mut req).is_ok());

        fn test(req: &mut dyn RequestExt) -> HttpResult {
            assert!(req.cookies().get("foo").is_some());
            let body: Body = Box::new(std::io::empty());
            Response::builder().body(body)
        }
    }

    #[test]
    fn set_cookie() {
        let mut req = MockRequest::new(Method::POST, "/articles");
        let mut app = MiddlewareBuilder::new(test);
        app.add(Middleware::new());
        let response = app.call(&mut req).ok().unwrap();
        let v = &response
            .headers()
            .get_all(header::SET_COOKIE)
            .iter()
            .collect::<Vec<_>>();
        assert_eq!(&v[..], ["foo=bar"]);

        fn test(req: &mut dyn RequestExt) -> HttpResult {
            let c = Cookie::new("foo".to_string(), "bar".to_string());
            req.cookies_mut().add(c);
            let body: Body = Box::new(std::io::empty());
            Response::builder().body(body)
        }
    }

    #[test]
    fn cookie_list() {
        let mut req = MockRequest::new(Method::POST, "/articles");
        let mut app = MiddlewareBuilder::new(test);
        app.add(Middleware::new());
        let response = app.call(&mut req).ok().unwrap();
        let mut v = response
            .headers()
            .get_all(header::SET_COOKIE)
            .iter()
            .collect::<Vec<_>>();
        v.sort();
        assert_eq!(&v[..], ["baz=qux", "foo=bar"]);

        fn test(req: &mut dyn RequestExt) -> HttpResult {
            let c = Cookie::new("foo", "bar");
            req.cookies_mut().add(c);
            let c2 = Cookie::new("baz", "qux");
            req.cookies_mut().add(c2);
            let body: Body = Box::new(std::io::empty());
            Response::builder().body(body)
        }
    }
}
