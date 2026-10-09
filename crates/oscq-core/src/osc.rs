//! OSC argument coercion, encoding and UDP sending.

use std::net::SocketAddr;

use rosc::{OscArray, OscColor, OscMessage, OscPacket, OscType};
use tokio::net::UdpSocket;

use crate::host_info::{OscDestination, SendBlocked};
use crate::model::parse_rgba_hex;
use crate::path::OscPath;
use crate::types::{ArgType, TypeSignature, TypeTag};

#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum CoerceError {
    #[error("expected {expected} arguments, got {got}")]
    Arity { expected: usize, got: usize },
    #[error("argument {index}: cannot convert {got} to OSC type '{tag}'")]
    Mismatch {
        index: usize,
        tag: char,
        got: String,
    },
    #[error("argument {index}: {value} is out of range for OSC type '{tag}'")]
    OutOfRange {
        index: usize,
        tag: char,
        value: String,
    },
    #[error("OSC type '{0}' cannot be encoded by this client")]
    Unencodable(char),
    #[error("argument {index}: expected an array for a '[...]' group")]
    ExpectedArray { index: usize },
}

#[derive(Debug, thiserror::Error)]
pub enum OscError {
    #[error(transparent)]
    Blocked(#[from] SendBlocked),
    #[error(transparent)]
    Coerce(#[from] CoerceError),
    #[error("OSC encoding failed: {0}")]
    Encode(String),
    #[error("cannot resolve OSC destination {host}:{port}: {source}")]
    Resolve {
        host: String,
        port: u16,
        source: std::io::Error,
    },
    #[error("UDP send failed: {0}")]
    Io(#[from] std::io::Error),
}

/// Coerces arguments to the declared signature, so that a declared int is never sent as a float.
///
/// Numbers convert between numeric tags. A non integral number sent to `i` or `h` is rounded to
/// the nearest integer, ties away from zero, then range checked. `T` and `F` accept either
/// boolean, as the proposal allows. `r` accepts a colour or a `#RRGGBBAA` string. `c` accepts a
/// one character string. `S` is refused because `rosc` cannot encode it.
pub fn coerce_args(sig: &TypeSignature, args: Vec<OscType>) -> Result<Vec<OscType>, CoerceError> {
    coerce_list(sig.args(), args, 0)
}

fn coerce_list(
    types: &[ArgType],
    args: Vec<OscType>,
    base: usize,
) -> Result<Vec<OscType>, CoerceError> {
    if types.len() != args.len() {
        return Err(CoerceError::Arity {
            expected: types.len(),
            got: args.len(),
        });
    }
    types
        .iter()
        .zip(args)
        .enumerate()
        .map(|(i, (ty, arg))| coerce_one(ty, arg, base + i))
        .collect()
}

fn coerce_one(ty: &ArgType, arg: OscType, index: usize) -> Result<OscType, CoerceError> {
    let tag = match ty {
        ArgType::Array(inner) => {
            return match arg {
                OscType::Array(a) => Ok(OscType::Array(OscArray {
                    content: coerce_list(inner, a.content, 0)?,
                })),
                _ => Err(CoerceError::ExpectedArray { index }),
            };
        }
        ArgType::Tag(tag) => *tag,
    };
    let c = tag.as_char();
    let mismatch = |got: &OscType| CoerceError::Mismatch {
        index,
        tag: c,
        got: format!("{got:?}"),
    };
    let out_of_range = |value: String| CoerceError::OutOfRange {
        index,
        tag: c,
        value,
    };
    Ok(match (tag, arg) {
        (TypeTag::Int32, a) => {
            let n = as_rounded_i64(&a).ok_or_else(|| mismatch(&a))?;
            OscType::Int(i32::try_from(n).map_err(|_| out_of_range(n.to_string()))?)
        }
        (TypeTag::Int64, a) => OscType::Long(as_rounded_i64(&a).ok_or_else(|| mismatch(&a))?),
        (TypeTag::Float32, a) => OscType::Float(as_f64(&a).ok_or_else(|| mismatch(&a))? as f32),
        (TypeTag::Double, a) => OscType::Double(as_f64(&a).ok_or_else(|| mismatch(&a))?),
        (TypeTag::String, OscType::String(s)) => OscType::String(s),
        (TypeTag::Char, OscType::Char(ch)) => OscType::Char(ch),
        (TypeTag::Char, OscType::String(s)) => {
            let mut chars = s.chars();
            match (chars.next(), chars.next()) {
                (Some(ch), None) => OscType::Char(ch),
                _ => return Err(mismatch(&OscType::String(s))),
            }
        }
        (TypeTag::Color, OscType::Color(col)) => OscType::Color(col),
        (TypeTag::Color, OscType::String(s)) => match parse_rgba_hex(&s) {
            Some([red, green, blue, alpha]) => OscType::Color(OscColor {
                red,
                green,
                blue,
                alpha,
            }),
            None => return Err(mismatch(&OscType::String(s))),
        },
        (TypeTag::True | TypeTag::False, OscType::Bool(b)) => OscType::Bool(b),
        (TypeTag::Nil, OscType::Nil) => OscType::Nil,
        (TypeTag::Impulse, OscType::Inf) => OscType::Inf,
        (TypeTag::Blob, OscType::Blob(b)) => OscType::Blob(b),
        (TypeTag::Midi, OscType::Midi(m)) => OscType::Midi(m),
        (TypeTag::TimeTag, OscType::Time(t)) => OscType::Time(t),
        (TypeTag::Symbol, _) => return Err(CoerceError::Unencodable('S')),
        (_, a) => return Err(mismatch(&a)),
    })
}

fn as_f64(a: &OscType) -> Option<f64> {
    let v = match a {
        OscType::Int(i) => f64::from(*i),
        OscType::Long(l) => *l as f64,
        OscType::Float(f) => f64::from(*f),
        OscType::Double(d) => *d,
        _ => return None,
    };
    v.is_finite().then_some(v)
}

fn as_rounded_i64(a: &OscType) -> Option<i64> {
    match a {
        OscType::Int(i) => Some(i64::from(*i)),
        OscType::Long(l) => Some(*l),
        OscType::Float(_) | OscType::Double(_) => {
            let r = as_f64(a)?.round();
            // i64::MAX as f64 rounds up to 2^63, which is out of range.
            (r >= i64::MIN as f64 && r < i64::MAX as f64).then_some(r as i64)
        }
        _ => None,
    }
}

/// Parses one argument typed as text, for the raw entry field and command line tools.
/// `T`, `F`, `N`, `I` ignore the text; `T` and `F` also accept `true`/`false`/`1`/`0`.
pub fn arg_from_text(ty: &ArgType, text: &str) -> Result<OscType, CoerceError> {
    let ArgType::Tag(tag) = ty else {
        return Err(CoerceError::ExpectedArray { index: 0 });
    };
    let mismatch = || CoerceError::Mismatch {
        index: 0,
        tag: tag.as_char(),
        got: format!("{text:?}"),
    };
    let t = text.trim();
    Ok(match tag {
        // A decimal number for an integer tag is left to `coerce_args`, which rounds it.
        TypeTag::Int32 | TypeTag::Int64 => match t.parse::<i64>() {
            Ok(n) => OscType::Long(n),
            Err(_) => OscType::Double(t.parse().map_err(|_| mismatch())?),
        },
        TypeTag::Float32 | TypeTag::Double => {
            let n: f64 = t.parse().map_err(|_| mismatch())?;
            OscType::Double(n)
        }
        TypeTag::String | TypeTag::Char | TypeTag::Color => OscType::String(text.to_owned()),
        TypeTag::True | TypeTag::False => match t {
            "" if *tag == TypeTag::True => OscType::Bool(true),
            "" => OscType::Bool(false),
            "true" | "1" | "T" => OscType::Bool(true),
            "false" | "0" | "F" => OscType::Bool(false),
            _ => return Err(mismatch()),
        },
        TypeTag::Nil => OscType::Nil,
        TypeTag::Impulse => OscType::Inf,
        TypeTag::Symbol => return Err(CoerceError::Unencodable('S')),
        TypeTag::Blob | TypeTag::Midi | TypeTag::TimeTag => {
            return Err(CoerceError::Unencodable(tag.as_char()));
        }
    })
}

/// Encodes one OSC message.
pub fn encode_message(path: &OscPath, args: Vec<OscType>) -> Result<Vec<u8>, OscError> {
    rosc::encoder::encode(&OscPacket::Message(OscMessage {
        addr: path.as_str().to_owned(),
        args,
    }))
    .map_err(|e| OscError::Encode(e.to_string()))
}

/// Sends OSC over UDP from one ephemeral socket per address family.
#[derive(Debug, Default)]
pub struct OscUdpSender {
    v4: Option<UdpSocket>,
    v6: Option<UdpSocket>,
}

impl OscUdpSender {
    pub fn new() -> Self {
        Self::default()
    }

    /// Resolves the destination, preferring IPv4, and sends `bytes` to it. Refuses destinations
    /// this client cannot send to.
    pub async fn send_to(
        &mut self,
        dest: &OscDestination,
        bytes: &[u8],
    ) -> Result<SocketAddr, OscError> {
        dest.check_sendable()?;
        let addr = resolve(&dest.host, dest.port).await?;
        let socket = self.socket_for(&addr).await?;
        socket.send_to(bytes, addr).await?;
        Ok(addr)
    }

    /// Coerces, encodes and sends one message.
    pub async fn send_message(
        &mut self,
        dest: &OscDestination,
        path: &OscPath,
        sig: &TypeSignature,
        args: Vec<OscType>,
    ) -> Result<SocketAddr, OscError> {
        dest.check_sendable()?;
        let args = coerce_args(sig, args)?;
        let bytes = encode_message(path, args)?;
        self.send_to(dest, &bytes).await
    }

    async fn socket_for(&mut self, addr: &SocketAddr) -> std::io::Result<&UdpSocket> {
        let (slot, bind) = if addr.is_ipv4() {
            (&mut self.v4, "0.0.0.0:0")
        } else {
            (&mut self.v6, "[::]:0")
        };
        if slot.is_none() {
            *slot = Some(UdpSocket::bind(bind).await?);
        }
        slot.as_ref()
            .ok_or_else(|| std::io::Error::other("socket not initialised"))
    }
}

async fn resolve(host: &str, port: u16) -> Result<SocketAddr, OscError> {
    let resolve_err = |source| OscError::Resolve {
        host: host.to_owned(),
        port,
        source,
    };
    let addrs: Vec<SocketAddr> = tokio::net::lookup_host((host, port))
        .await
        .map_err(resolve_err)?
        .collect();
    addrs
        .iter()
        .find(|a| a.is_ipv4())
        .or_else(|| addrs.first())
        .copied()
        .ok_or_else(|| resolve_err(std::io::Error::other("no address")))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sig(s: &str) -> TypeSignature {
        TypeSignature::parse(s).unwrap()
    }

    #[test]
    fn never_sends_a_float_to_an_int() {
        let out = coerce_args(&sig("i"), vec![OscType::Float(2.6)]).unwrap();
        assert_eq!(out, vec![OscType::Int(3)]);
        let out = coerce_args(&sig("i"), vec![OscType::Double(-2.5)]).unwrap();
        assert_eq!(out, vec![OscType::Int(-3)]);
        let out = coerce_args(&sig("h"), vec![OscType::Int(7)]).unwrap();
        assert_eq!(out, vec![OscType::Long(7)]);
    }

    #[test]
    fn numbers_to_floats() {
        let out = coerce_args(&sig("fd"), vec![OscType::Int(1), OscType::Long(2)]).unwrap();
        assert_eq!(out, vec![OscType::Float(1.0), OscType::Double(2.0)]);
    }

    #[test]
    fn rejects_out_of_range_and_non_finite() {
        assert!(matches!(
            coerce_args(&sig("i"), vec![OscType::Long(1 << 40)]),
            Err(CoerceError::OutOfRange { .. })
        ));
        assert!(matches!(
            coerce_args(&sig("f"), vec![OscType::Double(f64::NAN)]),
            Err(CoerceError::Mismatch { .. })
        ));
        assert!(matches!(
            coerce_args(&sig("h"), vec![OscType::Double(1e30)]),
            Err(CoerceError::Mismatch { .. })
        ));
    }

    #[test]
    fn true_and_false_are_interchangeable() {
        let out = coerce_args(&sig("T"), vec![OscType::Bool(false)]).unwrap();
        assert_eq!(out, vec![OscType::Bool(false)]);
        let out = coerce_args(&sig("F"), vec![OscType::Bool(true)]).unwrap();
        assert_eq!(out, vec![OscType::Bool(true)]);
    }

    #[test]
    fn colour_from_hex_string() {
        let out = coerce_args(&sig("r"), vec![OscType::String("#FA6432FF".into())]).unwrap();
        assert_eq!(
            out,
            vec![OscType::Color(OscColor {
                red: 250,
                green: 100,
                blue: 50,
                alpha: 255
            })]
        );
    }

    #[test]
    fn arity_arrays_and_symbol() {
        assert!(matches!(
            coerce_args(&sig("ff"), vec![OscType::Float(1.0)]),
            Err(CoerceError::Arity {
                expected: 2,
                got: 1
            })
        ));
        let out = coerce_args(
            &sig("i[ff]"),
            vec![
                OscType::Int(1),
                OscType::Array(OscArray {
                    content: vec![OscType::Int(1), OscType::Double(0.5)],
                }),
            ],
        )
        .unwrap();
        assert_eq!(
            out[1],
            OscType::Array(OscArray {
                content: vec![OscType::Float(1.0), OscType::Float(0.5)]
            })
        );
        assert_eq!(
            coerce_args(&sig("S"), vec![OscType::String("x".into())]),
            Err(CoerceError::Unencodable('S'))
        );
        assert_eq!(
            coerce_args(&sig("s"), vec![OscType::Int(1)])
                .map_err(|e| matches!(e, CoerceError::Mismatch { tag: 's', .. })),
            Err(true)
        );
    }

    #[test]
    fn text_arguments() {
        let t = |s: &str, txt: &str| arg_from_text(&sig(s).args()[0], txt);
        assert_eq!(t("i", "42").unwrap(), OscType::Long(42));
        assert_eq!(t("i", "4.4").unwrap(), OscType::Double(4.4));
        assert_eq!(t("f", "0.5").unwrap(), OscType::Double(0.5));
        assert_eq!(t("T", "").unwrap(), OscType::Bool(true));
        assert_eq!(t("F", "true").unwrap(), OscType::Bool(true));
        assert_eq!(t("I", "").unwrap(), OscType::Inf);
        assert!(t("i", "x").is_err());
    }

    #[test]
    fn encodes_a_decodable_message() {
        let path = OscPath::parse("/layer1/opacity").unwrap();
        let bytes = encode_message(&path, vec![OscType::Float(0.82)]).unwrap();
        let (_, packet) = rosc::decoder::decode_udp(&bytes).unwrap();
        assert_eq!(
            packet,
            OscPacket::Message(OscMessage {
                addr: "/layer1/opacity".into(),
                args: vec![OscType::Float(0.82)]
            })
        );
    }
}
