
#[derive(Debug, Clone, Default)]
pub struct BlockInfo {
    pub number: String
}

impl BlockInfo {

    // Create a new `BlockInfo` instance
    pub fn new(number: String) -> Self {
        Self {
            number
        }
    }

}
