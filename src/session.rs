use base64::{decode, encode};
use std::collections::HashMap;
use std::str;

use conduit::RequestExt;
use conduit_middleware::{AfterResult, BeforeResult};
use cookie::{Cookie, Key, SameSite};

use super::RequestCookies;

const MAX_AGE_DAYS: i64 = 90;

pub struct SessionMiddleware {
    cookie_name: String,
    key: Key,
    secure: bool,
}

pub struct Session {
    data: HashMap<String, String>,
    dirty: bool,
}

impl SessionMiddleware {
    pub fn new(cookie: &str, key: Key, secure: bool) -> SessionMiddleware {
        SessionMiddleware {
            cookie_name: cookie.to_string(),
            key,
            secure,
        }
    }

    pub fn decode(cookie: Cookie<'_>) -> HashMap<String, String> {
        let mut ret = HashMap::new();
        let bytes = decode(cookie.value().as_bytes()).unwrap_or_default();
        let mut parts = bytes.split(|&a| a == 0xff);
        while let (Some(key), Some(value)) = (parts.next(), parts.next()) {
            if key.is_empty() {
                break;
            }
            if let (Ok(key), Ok(value)) = (str::from_utf8(key), str::from_utf8(value)) {
                ret.insert(key.to_string(), value.to_string());
            }
        }
        ret
    }

    pub fn encode(h: &HashMap<String, String>) -> String {
        let mut ret = Vec::new();
        for (i, (k, v)) in h.iter().enumerate() {
            if i != 0 {
                ret.push(0xff)
            }
            ret.extend(k.bytes());
            ret.push(0xff);
            ret.extend(v.bytes());
        }
        while ret.len() * 8 % 6 != 0 {
            ret.push(0xff);
        }
        encode(&ret[..])
    }
}

impl conduit_middleware::Middleware for SessionMiddleware {
    fn before(&self, req: &mut dyn RequestExt) -> BeforeResult {
        let session = {
            let jar = req.cookies_mut().signed(&self.key);
            jar.get(&self.cookie_name)
                .map(Self::decode)
                .unwrap_or_else(HashMap::new)
        };
        req.mut_extensions().insert(Session {
            data: session,
            dirty: false,
        });
        Ok(())
    }

    fn after(&self, req: &mut dyn RequestExt, res: AfterResult) -> AfterResult {
        let session = req.extensions().find::<Session>();
        let session = session.expect("session must be present after request");
        if session.dirty {
            let encoded = Self::encode(&session.data);
            let cookie = Cookie::build(self.cookie_name.to_string(), encoded)
                .http_only(true)
                .secure(self.secure)
                .same_site(SameSite::Strict)
                .max_age(time::Duration::days(MAX_AGE_DAYS))
                .path("/")
                .finish();
            req.cookies_mut().signed_mut(&self.key).add(cookie);
        }
        res
    }
}

pub trait RequestSession {
    fn session(&self) -> &HashMap<String, String>;
    fn session_mut(&mut self) -> &mut HashMap<String, String>;
}

impl<T: RequestExt + ?Sized> RequestSession for T {
    fn session(&self) -> &HashMap<String, String> {
        &self
            .extensions()
            .find::<Session>()
            .expect("missing cookie session")
            .data
    }

    fn session_mut(&mut self) -> &mut HashMap<String, String> {
        let session = self
            .mut_extensions()
            .find_mut::<Session>()
            .expect("missing cookie session");
        session.dirty = true;
        &mut session.data
    }
}

#[cfg(test)]
mod test {
    use std::collections::HashMap;

    use conduit::{header, Body, Handler, HttpResult, Method, RequestExt, Response};
    use conduit_middleware::MiddlewareBuilder;
    use conduit_test::MockRequest;
    use cookie::{Cookie, Key};

    use {Middleware, RequestSession, SessionMiddleware};

    fn test_key() -> Key {
        let master_key: Vec<u8> = (0..32).collect();
        Key::derive_from(&master_key)
    }

    #[test]
    fn simple() {
        let mut req = MockRequest::new(Method::POST, "/articles");
        let key = test_key();

        // Set the session cookie
        let mut app = MiddlewareBuilder::new(set_session);
        app.add(Middleware::new());
        app.add(SessionMiddleware::new("lol", key, false));
        let response = app.call(&mut req).unwrap();

        let v = response
            .headers()
            .get(header::SET_COOKIE)
            .unwrap()
            .to_str()
            .unwrap();
        assert!(v.starts_with("lol"));

        // Use the session cookie
        req.header(header::COOKIE, v);
        let key = test_key();
        let mut app = MiddlewareBuilder::new(use_session);
        app.add(Middleware::new());
        app.add(SessionMiddleware::new("lol", key, false));
        assert!(app.call(&mut req).is_ok());

        fn set_session(req: &mut dyn RequestExt) -> HttpResult {
            assert!(req
                .session_mut()
                .insert("foo".to_string(), "bar".to_string())
                .is_none());
            Response::builder().body(Body::empty())
        }
        fn use_session(req: &mut dyn RequestExt) -> HttpResult {
            assert_eq!(*req.session().get("foo").unwrap(), "bar");
            Response::builder().body(Body::empty())
        }
    }

    #[test]
    fn no_equals() {
        let e = {
            let mut map = HashMap::new();
            map.insert("a".to_string(), "bc".to_string());
            SessionMiddleware::encode(&map)
        };
        assert!(!e.ends_with('='));

        let m = SessionMiddleware::decode(Cookie::new("foo", e));
        assert_eq!(*m.get("a").unwrap(), "bc");
    }

    #[test]
    fn dirty_tracking() {
        let mut req = MockRequest::new(Method::GET, "/");

        let mut app = MiddlewareBuilder::new(read_session);
        app.add(Middleware::new());
        app.add(SessionMiddleware::new("dirty", test_key(), false));
        let response = app.call(&mut req).unwrap();

        assert!(response.headers().get(header::SET_COOKIE).is_none());

        let mut app = MiddlewareBuilder::new(modify_session);
        app.add(Middleware::new());
        app.add(SessionMiddleware::new("dirty", test_key(), false));
        let response = app.call(&mut req).unwrap();

        assert!(response.headers().get(header::SET_COOKIE).is_some());

        fn read_session(req: &mut dyn RequestExt) -> HttpResult {
            req.session();
            Response::builder().body(Body::empty())
        }
        fn modify_session(req: &mut dyn RequestExt) -> HttpResult {
            req.session_mut();
            Response::builder().body(Body::empty())
        }
    }
}
