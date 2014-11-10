#![feature(macro_rules, globs)]

extern crate conduit;
extern crate "conduit-middleware" as conduit_middleware;
extern crate cookie;
extern crate serialize;
#[cfg(test)] extern crate "conduit-test" as test;

use std::fmt::Show;
use std::collections::hash_map::{Occupied, Vacant};
use conduit::{Request, Response};
use cookie::{CookieJar, Cookie};

pub use session::{RequestSession, SessionMiddleware};

mod session;

pub struct Middleware {
    key: Vec<u8>,
}

impl Middleware {
    pub fn new(key: &[u8]) -> Middleware {
        Middleware { key: key.to_vec() }
    }
}

impl conduit_middleware::Middleware for Middleware {
    fn before(&self, req: &mut Request) -> Result<(), Box<Show + 'static>> {
        let jar = {
            let headers = req.headers();
            let mut jar = CookieJar::new(self.key.as_slice());
            match headers.find("Cookie") {
                Some(cookies) => {
                    for cookie in cookies.iter() {
                        match Cookie::parse(*cookie) {
                            Ok(c) => jar.add_original(c),
                            Err(..) => {}
                        }
                    }
                }
                None => {}
            }
            jar
        };
        req.mut_extensions().insert(jar);
        Ok(())
    }

    fn after(&self, req: &mut Request, res: Result<Response, Box<Show + 'static>>)
        -> Result<Response, Box<Show + 'static>>
    {
        let mut res = try!(res);
        {
            let jar = req.cookies();
            let cookies = match res.headers.entry("Set-Cookie".to_string()) {
                Occupied(e) => e.into_mut(),
                Vacant(e) => e.set(Vec::new()),
            };
            for delta in jar.delta().into_iter() {
                cookies.push(delta.to_string());
            }
        }
        Ok(res)
    }
}

pub trait RequestCookies<'a> {
    fn cookies(self) -> &'a CookieJar<'static>;
}

impl<'a> RequestCookies<'a> for &'a Request + 'a {
    fn cookies(self) -> &'a CookieJar<'static> {
        self.extensions().find::<CookieJar<'static>>()
            .expect("Missing cookie jar")
    }
}

#[cfg(test)]
mod tests {

    use conduit::{Request, Response, Handler, Post};
    use conduit_middleware::MiddlewareBuilder;
    use cookie::Cookie;
    use test::MockRequest;
    use std::collections::HashMap;
    use std::io::MemReader;

    use super::{RequestCookies, Middleware};

    #[test]
    fn request_headers() {
        let mut req = MockRequest::new(Post, "/articles");
        req.header("Cookie", "foo=bar");

        let mut app = MiddlewareBuilder::new(test);
        app.add(Middleware::new(b"foo"));
        assert!(app.call(&mut req).is_ok());

        fn test(req: &mut Request) -> Result<Response, String> {
            assert!(req.cookies().find("foo").is_some());
            Ok(Response {
                status: (200, "OK"),
                headers: HashMap::new(),
                body: box MemReader::new(Vec::new()),
            })
        }
    }

    #[test]
    fn set_cookie() {
        let mut req = MockRequest::new(Post, "/articles");
        let mut app = MiddlewareBuilder::new(test);
        app.add(Middleware::new(b"foo"));
        let response = app.call(&mut req).ok().unwrap();
        let v = response.headers["Set-Cookie".to_string()].as_slice();
        assert_eq!(v, ["foo=bar; Path=/".to_string()].as_slice());

        fn test(req: &mut Request) -> Result<Response, String> {
            let c = Cookie::new("foo".to_string(), "bar".to_string());
            req.cookies().add(c);
            Ok(Response {
                status: (200, "OK"),
                headers: HashMap::new(),
                body: box MemReader::new(Vec::new()),
            })
        }
    }
}
