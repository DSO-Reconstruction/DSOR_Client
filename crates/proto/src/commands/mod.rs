//! Game commands (the 0x84 single and 0x85 chained server commands, the 0x8B client
//! commands). Each command is decoded field by field: a 0x85 batch carries no lengths,
//! so the next command can only be found by reading the previous one entirely.
