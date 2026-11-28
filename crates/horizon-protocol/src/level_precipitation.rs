//! Clientbound play state for one named level's live precipitation policy.
use crate::{
    Error, Result,
    buf::{Reader, Writer},
    packets::{configuration::CustomPayload, play::Clientbound},
};

pub const CHANNEL: &str = "horizon:level_precipitation";

pub fn packet(level: &str, force_snow: bool) -> Clientbound {
    let mut writer = Writer::new();
    writer.string(level);
    writer.u8(u8::from(force_snow));
    Clientbound::CustomPayload(CustomPayload {
        channel: CHANNEL.into(),
        data: writer.data,
    })
}

pub fn decode(data: &[u8]) -> Result<(String, bool)> {
    let mut reader = Reader::new(data);
    let level = reader.string(32_767)?;
    let force = reader.u8()?;
    if force > 1 || reader.remaining() != 0 {
        return Err(Error::new("invalid level precipitation state"));
    }
    Ok((level, force != 0))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn level_precipitation_round_trips_in_play() {
        let level = "test:winter/overworld";
        for force in [false, true] {
            let bytes = packet(level, force).encode();
            let mut reader = Reader::new(&bytes);
            let id = reader.varint().unwrap();
            let Clientbound::CustomPayload(payload) =
                Clientbound::decode(id, reader.rest()).unwrap()
            else {
                panic!("expected a custom payload");
            };
            assert_eq!(payload.channel, CHANNEL);
            assert_eq!(decode(&payload.data).unwrap(), (level.to_owned(), force));

            let mut invalid = payload.data;
            assert!(decode(&invalid[..invalid.len() - 1]).is_err());
            *invalid.last_mut().unwrap() = 2;
            assert!(decode(&invalid).is_err());
            *invalid.last_mut().unwrap() = 1;
            invalid.push(0);
            assert!(decode(&invalid).is_err());
        }
    }
}
