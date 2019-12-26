pub mod message;
pub use message::{Offer, OfferProtocol, Use, UseProtocol};

use super::{FramedStream, MatchProtocol, ProtocolHandler, Switch};
use crate::network::{Protocol, ProtocolId};

use anyhow::{Context, Error};
use async_trait::async_trait;
use bytes::BytesMut;
use futures::{lock::Mutex, stream::TryStreamExt, SinkExt};
use tracing::{debug, trace_span};

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
        let proto = Protocol::new(*handler.proto_id(), handler.proto_name());
        debug!("add protocol {} handler", proto);

        let reg_proto = RegisteredProtocol {
            inner: proto,
            r#match: None,
            handler: Box::new(handler),
        };

        {
            self.register.lock().await.insert(reg_proto)
        };

        Ok(())
    }

    async fn add_match_handler(
        &self,
        r#match: impl for<'a> MatchProtocol<'a> + Send + 'static,
        handler: impl ProtocolHandler + 'static,
    ) -> Result<(), Error> {
        let proto = Protocol::new(*handler.proto_id(), handler.proto_name());
        debug!("add protocol {} handler", proto);

        let reg_proto = RegisteredProtocol {
            inner: proto,
            r#match: Some(Box::new(r#match)),
            handler: Box::new(handler),
        };

        {
            self.register.lock().await.insert(reg_proto)
        };

        Ok(())
    }

    async fn remove_handler(&self, proto_id: ProtocolId) {
        debug!("remove protocol {} handler", proto_id);

        self.register.lock().await.remove(&proto_id);
    }

    async fn negotiate(
        &self,
        stream: &mut FramedStream,
    ) -> Result<Box<dyn ProtocolHandler>, Error> {
        use prost::Message;
        use SwitchError::*;

        let span = trace_span!("negotiate incoming stream");
        let _guard = span.enter();

        let first_msg = stream.try_next().await?.ok_or(NoProtocolOffer)?;
        debug!("first msg");

        let offer = Offer::decode(first_msg).context("first got message must be offer")?;
        let protocols_in_offer = offer.into_protocols();
        debug!("protocols in offer: {:?}", protocols_in_offer);

        if protocols_in_offer.is_empty() {
            return Err(EmptyOffer.into());
        }

        let register = { self.register.lock().await.clone() };
        let proto_handler = match_protocol(register, protocols_in_offer).ok_or(NoProtocolMatch)?;

        let r#use = Use::new(
            *proto_handler.proto_id(),
            proto_handler.proto_name().to_owned(),
        );
        let mut use_data = BytesMut::new();
        r#use.encode(&mut use_data)?;

        stream.send(use_data.freeze()).await?;

        Ok(proto_handler)
    }
}

fn match_protocol(
    register: HashSet<RegisteredProtocol>,
    protocols_in_offer: Vec<OfferProtocol>,
) -> Option<Box<dyn ProtocolHandler>> {
    for protocol in protocols_in_offer.into_iter() {
        match protocol {
            OfferProtocol::Id(id) => {
                let id: ProtocolId = id.into();

                if let Some(reg_proto) = register.get(&id) {
                    return Some(dyn_clone::clone_box(&*reg_proto.handler));
                }
            }
            OfferProtocol::Name(name) => {
                for reg_proto in register.iter() {
                    if reg_proto.inner.name == name {
                        return Some(dyn_clone::clone_box(&*reg_proto.handler));
                    }

                    if let Some(proto_match) = reg_proto.r#match.as_ref() {
                        if proto_match.r#match(&name) {
                            return Some(dyn_clone::clone_box(&*reg_proto.handler));
                        }
                    }
                }
            }
        }
    }

    None
}
