pub(crate) use habeas_sdk::redaction::*;

#[cfg(test)]
pub(crate) mod tests {
    /// A GitHub classic token shape used by frontend integration tests.
    // {cite}`habeas:util`
    pub(crate) fn github_token() -> String {
        format!("gh{}_{}", "p", "16C7e42F292c6912E7710c838347Ae178B4a")
    }
}
