use std::collections::HashMap;
use std::error::Error;
use std::str;
use rustc_serialize::base64::{FromBase64, ToBase64, STANDARD};

use conduit::{Request, Response};
use conduit_middleware;
use cookie::Cookie;

use super::RequestCookies;

pub struct SessionMiddleware {
    cookie_name: String,
    secure: bool,
}

pub struct Session {
    pub data: HashMap<String, String>,
}

impl SessionMiddleware {
    pub fn new(cookie: &str, secure: bool) -> SessionMiddleware {
        SessionMiddleware {
            cookie_name: cookie.to_string(),
            secure: secure,
        }
    }

    pub fn decode(&self, cookie: Cookie) -> HashMap<String, String> {
        let mut ret = HashMap::new();
        let bytes = cookie.value.from_base64().unwrap_or(Vec::new());
        let mut parts = bytes.split(|&a| a == 0xff);
        loop {
            match (parts.next(), parts.next()) {
                (Some(key), Some(value)) => {
                    if key.len() == 0 { break }
                    match (str::from_utf8(key), str::from_utf8(value)) {
                        (Ok(key), Ok(value)) => {
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
            ret.extend(k.bytes());
            ret.push(0xff);
            ret.extend(v.bytes());
        }
        while ret.len() * 8 % 6 != 0 {
            ret.push(0xff);
        }
        ret.to_base64(STANDARD)
    }
}

impl conduit_middleware::Middleware for SessionMiddleware {
    fn before(&self, req: &mut Request) -> Result<(), Box<Error+Send>> {
        let session = {
            let jar = req.cookies().signed();
            jar.find(&self.cookie_name).map(|cookie| {
                self.decode(cookie)
            }).unwrap_or_else(|| HashMap::new())
        };
        req.mut_extensions().insert(Session { data: session });
        Ok(())
    }

    fn after(&self, req: &mut Request, res: Result<Response, Box<Error+Send>>)
        -> Result<Response, Box<Error+Send>>
    {
        let mut cookie = {
            let session = req.mut_extensions().find::<Session>();
            let session = session.expect("session must be present after request");
            let encoded = self.encode(&session.data);
            Cookie::new(self.cookie_name.to_string(), encoded)
        };
        cookie.httponly = true;
        cookie.secure = self.secure;
        cookie.path = Some("/".to_string());
        req.cookies().signed().add(cookie);
        return res;
    }
}

pub trait RequestSession {
    fn session(&mut self) -> &mut HashMap<String, String>;
}

impl<'a> RequestSession for Request+'a {
    fn session(&mut self) -> &mut HashMap<String, String> {
        &mut self.mut_extensions().find_mut::<Session>()
                 .expect("missing cookie session").data
    }
}

#[cfg(test)]
mod test {

    use std::collections::HashMap;
    use std::io::{self, Cursor};

    use conduit::{Request, Response, Handler, Method};
    use conduit_middleware::MiddlewareBuilder;
    use cookie::Cookie;
    use conduit_test::MockRequest;

    use {RequestSession, Middleware, SessionMiddleware};

    #[test]
    fn simple() {
        let mut req = MockRequest::new(Method::Post, "/articles");

        // Set the session cookie
        let mut app = MiddlewareBuilder::new(set_session);
        app.add(Middleware::new(b"foo"));
        app.add(SessionMiddleware::new("lol", false));
        let response = app.call(&mut req).ok().unwrap();

        let v = &response.headers["Set-Cookie"];
        assert!(v[0].starts_with("lol"));

        // Use the session cookie
        req.header("Cookie", &v[0]);
        let mut app = MiddlewareBuilder::new(use_session);
        app.add(Middleware::new(b"foo"));
        app.add(SessionMiddleware::new("lol", false));
        assert!(app.call(&mut req).is_ok());

        fn set_session(req: &mut Request) -> io::Result<Response> {
            assert!(req.session().insert("foo".to_string(), "bar".to_string())
                                 .is_none());
            Ok(Response {
                status: (200, "OK"),
                headers: HashMap::new(),
                body: Box::new(Cursor::new(Vec::new())),
            })
        }
        fn use_session(req: &mut Request) -> io::Result<Response> {
            assert_eq!(*req.session().get("foo").unwrap(), "bar");
            Ok(Response {
                status: (200, "OK"),
                headers: HashMap::new(),
                body: Box::new(Cursor::new(Vec::new())),
            })
        }
    }

    #[test]
    fn no_equals() {
        let m = SessionMiddleware::new("test", false);
        let e = {
            let mut map = HashMap::new();
            map.insert("a".to_string(), "bc".to_string());
            m.encode(&map)
        };
        assert!(!e.ends_with("="));
        let m = m.decode(Cookie::new("foo".to_string(), e));
        assert_eq!(*m.get("a").unwrap(), "bc");
    }
}
