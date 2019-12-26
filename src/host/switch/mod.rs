pub mod message;
pub use message::{Offer, Use, UseProtocol, OfferProtocol};

use super::{ProtocolHandler, RawStream, Switch, MatchProtocol};
use crate::network::{Protocol, ProtocolId};

use async_trait::async_trait;
use anyhow::{Error, Context};
use futures::{lock::Mutex, stream::TryStreamExt};
use futures_codec::{Framed, LengthCodec};

use std::{
    borrow::Borrow,
    collections::HashSet,
    hash::{Hash, Hasher},
    sync::Arc,
};

#[derive(thiserror::Error, Debug)]
pub enum SwitchError {
    #[error("no protocol offer")]
    NoProtocolOffer,
    #[error("no protocol name")]
    NoProtocolName,
    #[error("offer without protocol")]
    EmptyOffer,
    #[error("no protocol match")]
    NoProtocolMatch,
}

struct RegisteredProtocol {
    inner: Protocol,
    r#match: Option<Box<dyn for<'a> MatchProtocol<'a>>>,
    handler: Box<dyn ProtocolHandler>,
}

impl Clone for RegisteredProtocol {
    fn clone(&self) -> Self {
        let cloned_match = self.r#match.as_ref().map(|m| dyn_clone::clone_box(&**m));

        RegisteredProtocol {
            inner: self.inner.clone(),
            r#match: cloned_match,
            handler: dyn_clone::clone_box(&*self.handler),
        }
    }
}

impl Borrow<ProtocolId> for RegisteredProtocol {
    fn borrow(&self) -> &ProtocolId {
        &self.inner.id
    }
}

impl PartialEq for RegisteredProtocol {
    fn eq(&self, other: &RegisteredProtocol) -> bool {
        self.inner.id == other.inner.id
    }
}

impl Eq for RegisteredProtocol {}

impl Hash for RegisteredProtocol {
    fn hash<H: Hasher>(&self, hasher: &mut H) {
        self.inner.id.hash(hasher)
    }
}

#[derive(Clone)]
pub struct DefaultSwitch {
    register: Arc<Mutex<HashSet<RegisteredProtocol>>>,
}

impl Default for DefaultSwitch {
    fn default() -> Self {
        DefaultSwitch {
            register: Default::default(),
        }
    }
}

#[async_trait]
impl Switch for DefaultSwitch {
    async fn add_handler(&self, handler: impl ProtocolHandler + 'static) -> Result<(), Error> {
        let reg_proto = RegisteredProtocol {
            inner: Protocol::new(*handler.proto_id(), handler.proto_name()),
            r#match: None,
            handler: Box::new(handler),
        };

        { self.register.lock().await.insert(reg_proto) };

        Ok(())
    }

    async fn add_match_handler(&self, r#match: impl for<'a> MatchProtocol<'a> + Send + 'static, handler: impl ProtocolHandler + 'static) -> Result<(), Error> {
        let reg_proto = RegisteredProtocol {
            inner: Protocol::new(*handler.proto_id(), handler.proto_name()),
            r#match: Some(Box::new(r#match)),
            handler: Box::new(handler),
        };

        { self.register.lock().await.insert(reg_proto) };

        Ok(())
    }

    async fn remove_handler(&self, proto_id: ProtocolId) {
        self.register.lock().await.remove(&proto_id);
    }

    async fn negotiate(&self, mut stream: &mut dyn RawStream) -> Result<Box<dyn ProtocolHandler>, Error> {
        use prost::Message;
        use SwitchError::*;

        let mut framed = Framed::new(&mut stream, LengthCodec);

        let first_msg = framed.try_next().await?.ok_or(NoProtocolOffer)?;
        let offer = Offer::decode(first_msg).context("first got message must be offer")?;
        let protocols_in_offer = offer.into_protocols();

        if protocols_in_offer.is_empty() {
            return Err(EmptyOffer.into());
        }

        let register = { self.register.lock().await.clone() };

        for protocol in protocols_in_offer.into_iter() {
            match protocol {
                OfferProtocol::Id(id) => {
                    let id: ProtocolId = id.into();

                    if let Some(reg_proto) = register.get(&id) {
                        return Ok(dyn_clone::clone_box(&*reg_proto.handler));
                    }
                }
                OfferProtocol::Name(name) => {
                    for reg_proto in register.iter() {
                        if reg_proto.inner.name == name {
                            return Ok(dyn_clone::clone_box(&*reg_proto.handler));
                        }

                        if let Some(proto_match) = reg_proto.r#match.as_ref() {
                            if proto_match.r#match(&name) {
                                return Ok(dyn_clone::clone_box(&*reg_proto.handler));
                            }
                        }
                    }
                }
            }
        }

        Err(NoProtocolMatch.into())
    }
}
