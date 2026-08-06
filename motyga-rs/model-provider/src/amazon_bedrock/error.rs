use motyga_api::ApiError;
use motyga_protocol::error::MotygaErr;
use http::StatusCode;

pub(super) const BEDROCK_EXPIRED_SIGNATURE_MESSAGE: &str = concat!(
    "Amazon Bedrock rejected the request because its AWS signature has expired. ",
    "Refresh your AWS credentials and retry. If `AWS_BEARER_TOKEN_BEDROCK` is set, ",
    "update or unset it, then restart Motyga",
);

pub(super) fn map_api_error(error: ApiError) -> MotygaErr {
    let mut error = motyga_api::map_api_error(error);
    if let MotygaErr::UnexpectedStatus(response) = &mut error
        && response.status == StatusCode::UNAUTHORIZED
        && response.body.contains("Signature expired:")
    {
        response.user_message = Some(BEDROCK_EXPIRED_SIGNATURE_MESSAGE.to_string());
    }
    error
}
