use std::collections::HashMap;
use std::fmt::Show;
use std::str;
use serialize::base64::{FromBase64, ToBase64, STANDARD};

use conduit::{Request, Response};
use conduit_middleware;
use cookie::Cookie;

use super::RequestCookies;

pub struct SessionMiddleware {
    cookie_name: String,
}

pub struct Session {
    pub data: HashMap<String, String>,
}

impl SessionMiddleware {
    pub fn new(cookie: &str) -> SessionMiddleware {
        SessionMiddleware { cookie_name: cookie.to_string() }
    }

    pub fn decode(&self, cookie: Cookie) -> HashMap<String, String> {
        let mut ret = HashMap::new();
        let bytes = cookie.value.as_slice().from_base64().unwrap_or(Vec::new());
        let mut parts = bytes.as_slice().split(|&a| a == 0xff);
        loop {
            match (parts.next(), parts.next()) {
                (Some(key), Some(value)) => {
                    match (str::from_utf8(key), str::from_utf8(value)) {
                        (Some(key), Some(value)) => {
                            ret.insert(key.to_string(), value.to_string());
                        }
                        _ => {}
                    }
                }
                _ => break
            }
        }
        return ret;
    }

    pub fn encode(&self, h: &HashMap<String, String>) -> String {
        let mut ret = Vec::new();
        for (i, (k, v)) in h.iter().enumerate() {
            if i != 0 { ret.push(0xff) }
            ret.push_all(k.as_bytes());
            ret.push(0xff);
            ret.push_all(v.as_bytes());
        }
        ret.as_slice().to_base64(STANDARD)
    }
}

impl conduit_middleware::Middleware for SessionMiddleware {
    fn before(&self, req: &mut Request) -> Result<(), Box<Show>> {
        let session = {
            let jar = req.cookies().signed();
            jar.find(self.cookie_name.as_slice()).map(|cookie| {
                self.decode(cookie)
            }).unwrap_or_else(|| HashMap::new())
        };
        req.mut_extensions().insert(Session { data: session });
        Ok(())
    }

    fn after(&self, req: &mut Request, res: Result<Response, Box<Show>>)
        -> Result<Response, Box<Show>>
    {
        let cookie = {
            let session = req.mut_extensions().find::<Session>();
            let session = session.expect("session must be present after request");
            let encoded = self.encode(&session.data);
            Cookie::new(self.cookie_name.to_string(), encoded)
        };
        req.cookies().signed().add(cookie);
        return res;
    }
}

pub trait RequestSession<'a> {
    fn session(self) -> &'a mut HashMap<String, String>;
}

impl<'a> RequestSession<'a> for &'a mut Request {
    fn session(self) -> &'a mut HashMap<String, String> {
        &mut self.mut_extensions().find_mut::<Session>()
                 .expect("missing cookie session").data
    }
}

#[cfg(test)]
mod test {

    use conduit::{Request, Response, Handler, Post};
    use conduit_middleware::MiddlewareBuilder;
    use test::MockRequest;
    use std::collections::HashMap;
    use std::io::MemReader;

    use {RequestSession, Middleware, SessionMiddleware};

    #[test]
    fn simple() {
        let mut req = MockRequest::new(Post, "/articles");

        // Set the session cookie
        let mut app = MiddlewareBuilder::new(set_session);
        app.add(Middleware::new(b"foo"));
        app.add(SessionMiddleware::new("lol"));
        let response = app.call(&mut req).ok().unwrap();

        let v = response.headers["Set-Cookie".to_string()].as_slice();
        assert!(v[0].as_slice().starts_with("lol"));

        // Use the session cookie
        req.header("Cookie", v.as_slice()[0].as_slice());
        let mut app = MiddlewareBuilder::new(use_session);
        app.add(Middleware::new(b"foo"));
        app.add(SessionMiddleware::new("lol"));
        assert!(app.call(&mut req).is_ok());

        fn set_session(req: &mut Request) -> Result<Response, String> {
            assert!(req.session().insert("foo".to_string(), "bar".to_string()));
            Ok(Response {
                status: (200, "OK"),
                headers: HashMap::new(),
                body: box MemReader::new(Vec::new()),
            })
        }
        fn use_session(req: &mut Request) -> Result<Response, String> {
            assert_eq!(req.session().find_equiv(&"foo").unwrap().as_slice(),
                       "bar");
            Ok(Response {
                status: (200, "OK"),
                headers: HashMap::new(),
                body: box MemReader::new(Vec::new()),
            })
        }
    }
}
