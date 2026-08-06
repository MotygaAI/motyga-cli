use super::*;

pub(super) fn environment_selection_error(err: MotygaErr) -> JSONRPCErrorError {
    match err {
        MotygaErr::InvalidRequest(message) => invalid_request(message),
        err => internal_error(format!("failed to validate environment selections: {err}")),
    }
}
