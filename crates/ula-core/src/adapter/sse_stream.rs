use asynchronous_codec::{BytesMut, Decoder, LinesCodec};
use log::{error, warn};

use crate::adapter::errors::APIError;

pub enum SSEItem {
    Data(serde_json::Value),
    Done,
}

pub struct SSEDecoder;

impl Decoder for SSEDecoder {
    type Item = SSEItem;
    type Error = APIError;
    fn decode(&mut self, src: &mut BytesMut) -> Result<Option<Self::Item>, Self::Error> {
        while let Some(line) = LinesCodec.decode(src)? {
            let result = match line.strip_suffix('\n').unwrap_or(&line) {
                // Data
                "data: [DONE]" => SSEItem::Done,
                s if s.starts_with("data: ") => {
                    let data: serde_json::Value = serde_json::from_str(&s[6..])
                        .inspect_err(|_| error!("Invalid JSON content {:?}", &s[6..]))?;

                    SSEItem::Data(data)
                }

                s if s.starts_with(":") => continue,
                "" => continue,

                s => {
                    warn!("Unsupported SSE line {s:?}");
                    continue;
                }
            };
            return Ok(Some(result));
        }
        Ok(None)
    }
}
