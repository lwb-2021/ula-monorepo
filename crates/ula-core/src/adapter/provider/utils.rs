use isahc::HttpClient;

use crate::adapter::APIError;

pub trait HttpExt: Sized {
    fn raise_for_status(self) -> Result<Self, APIError>;
}

impl<T> HttpExt for isahc::http::Response<T> {
    fn raise_for_status(self) -> Result<Self, APIError> {
        let status = self.status();

        if !status.is_success() {
            return APIError::from_status_code(status.as_u16()).raise();
        }

        Ok(self)
    }
}

pub async fn get_standard_session_creator(
    api_key_command: &str,
) -> Result<impl FnOnce() -> Result<isahc::HttpClient, isahc::Error>, APIError> {
    let api_key = get_api_key(api_key_command).await?;
    Ok(move || {
        HttpClient::builder()
            .default_header("Authorization", format!("Bearer {api_key}"))
            .build()
    })
}

async fn get_api_key(api_key_command: &str) -> Result<String, APIError> {
    let result = smol::process::Command::new("sh")
        .arg("-c")
        .arg(api_key_command)
        .output()
        .await?;

    if !result.status.success() {
        return APIError::Fatal {
            message: format!(
                "Unable to get API key: `{}` exits with code {}\n{}",
                api_key_command,
                result.status,
                String::from_utf8_lossy(&result.stderr)
            ),
        }
        .raise();
    }

    let key_not_trimmed = String::from_utf8_lossy(&result.stdout);
    let key = key_not_trimmed.trim();

    if !key
        .chars()
        .all(|b| ((32u8 as char)..=(126u8 as char)).contains(&b))
    {
        return Err(APIError::Fatal {
            message: format!("Invaild unprintable char in API key from `{api_key_command}`"),
        });
    }
    return Ok(key.to_string());
}
