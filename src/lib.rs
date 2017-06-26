#![cfg_attr(test, deny(warnings))]

extern crate conduit;
extern crate conduit_middleware;
extern crate cookie;
extern crate base64;
#[cfg(test)]
extern crate conduit_test;

use std::error::Error;
use std::collections::hash_map::Entry;
use conduit::{Request, Response};
use cookie::{CookieJar, Cookie};

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
    fn before(&self, req: &mut Request) -> Result<(), Box<Error + Send>> {
        let jar = {
            let headers = req.headers();
            let mut jar = CookieJar::new();
            if let Some(cookies) = headers.find("Cookie") {
                for cookie in cookies.iter() {
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

    fn after(
        &self,
        req: &mut Request,
        res: Result<Response, Box<Error + Send>>,
    ) -> Result<Response, Box<Error + Send>> {
        let mut res = res?;
        {
            let jar = req.cookies();
            let cookies = match res.headers.entry("Set-Cookie".to_string()) {
                Entry::Occupied(e) => e.into_mut(),
                Entry::Vacant(e) => e.insert(Vec::new()),
            };
            for delta in jar.delta() {
                cookies.push(delta.to_string());
            }
        }
        Ok(res)
    }
}

pub trait RequestCookies {
    fn cookies(&self) -> &CookieJar;
    fn cookies_mut(&mut self) -> &mut CookieJar;
}

impl<T: Request + ?Sized> RequestCookies for T {
    fn cookies(&self) -> &CookieJar {
        self.extensions().find::<CookieJar>().expect(
            "Missing cookie jar",
        )
    }

    fn cookies_mut(&mut self) -> &mut CookieJar {
        self.mut_extensions().find_mut::<CookieJar>().expect(
            "Missing cookie jar",
        )
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::io::{self, Cursor};

    use conduit::{Request, Response, Handler, Method};
    use conduit_middleware::MiddlewareBuilder;
    use cookie::Cookie;
    use conduit_test::MockRequest;

    use super::{RequestCookies, Middleware};

    #[test]
    fn request_headers() {
        let mut req = MockRequest::new(Method::Post, "/articles");
        req.header("Cookie", "foo=bar");

        let mut app = MiddlewareBuilder::new(test);
        app.add(Middleware::new());
        assert!(app.call(&mut req).is_ok());

        fn test(req: &mut Request) -> io::Result<Response> {
            assert!(req.cookies().get("foo").is_some());
            Ok(Response {
                status: (200, "OK"),
                headers: HashMap::new(),
                body: Box::new(Cursor::new(Vec::new())),
            })
        }
    }

    #[test]
    fn set_cookie() {
        let mut req = MockRequest::new(Method::Post, "/articles");
        let mut app = MiddlewareBuilder::new(test);
        app.add(Middleware::new());
        let response = app.call(&mut req).ok().unwrap();
        let v = &response.headers["Set-Cookie"];
        assert_eq!(&v[..], ["foo=bar".to_string()]);

        fn test(req: &mut Request) -> io::Result<Response> {
            let c = Cookie::new("foo".to_string(), "bar".to_string());
            req.cookies_mut().add(c);
            Ok(Response {
                status: (200, "OK"),
                headers: HashMap::new(),
                body: Box::new(Cursor::new(Vec::new())),
            })
        }
    }

    #[test]
    fn cookie_list() {
        let mut req = MockRequest::new(Method::Post, "/articles");
        let mut app = MiddlewareBuilder::new(test);
        app.add(Middleware::new());
        let response = app.call(&mut req).ok().unwrap();
        let mut v = response.headers["Set-Cookie"].clone();
        v.sort();
        assert_eq!(&v[..], ["baz=qux".to_string(), "foo=bar".to_string()]);

        fn test(req: &mut Request) -> io::Result<Response> {
            let c = Cookie::new("foo".to_string(), "bar".to_string());
            req.cookies_mut().add(c);
            let c2 = Cookie::new("baz".to_string(), "qux".to_string());
            req.cookies_mut().add(c2);
            Ok(Response {
                status: (200, "OK"),
                headers: HashMap::new(),
                body: Box::new(Cursor::new(Vec::new())),
            })
        }
    }
}
