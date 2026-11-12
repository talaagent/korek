//! Every stable diagnostic code korek itself emits. Codes are API:
//! `snake_case`, never reused. `docs/errors.md` documents each one (a test
//! keeps the two in sync).

/// `(code, one-line summary, severity is warning)`.
pub const ALL: &[(&str, &str, bool)] = &[
    (SYNTAX, "the file is not valid KDL v2", false),
    (IO, "a schema file or directory could not be read", false),
    (
        UNKNOWN_NODE,
        "a node the grammar does not know and no extension claims",
        false,
    ),
    (
        UNKNOWN_PROPERTY,
        "a property the grammar does not know and no extension claims",
        false,
    ),
    (
        UNKNOWN_TYPE,
        "a type reference to an undeclared type",
        false,
    ),
    (
        UNKNOWN_RULE,
        "a rule name inside `validate` that korek does not know",
        false,
    ),
    (
        DUPLICATE_NAME,
        "two items with the same name in one scope",
        false,
    ),
    (
        NAME_CLASH,
        "a declared name clashes with one korek generates",
        false,
    ),
    (INVALID_NAME, "a name that breaks the naming rules", false),
    (INVALID_TYPE, "a malformed type expression", false),
    (
        INVALID_VALUE,
        "a malformed argument or property value",
        false,
    ),
    (
        MISSING_VALUE,
        "a required argument or property is missing",
        false,
    ),
    (
        INVALID_DEFAULT,
        "a default that does not match its type or rules",
        false,
    ),
    (
        INVALID_EXAMPLE,
        "an example that does not match its type or rules",
        false,
    ),
    (
        INVALID_RULE,
        "a rule that does not apply to its type or has a bad operand",
        false,
    ),
    (RULE_UNSUPPORTED, "a generator cannot express a rule", true),
    (
        INVALID_UNION,
        "a tagged union with a non-struct variant or a clashing tag",
        false,
    ),
    (
        INVALID_RPC,
        "an rpc whose input or output is not a message",
        false,
    ),
    (CYCLE, "a recursive type (not supported yet)", false),
    (EMPTY, "an enum or union without variants", false),
    (INVALID_ID, "an explicit id out of range or reserved", false),
    (DUPLICATE_ID, "two items pinned to the same id", false),
    (ID_TAKEN, "an id the lockfile gives to another item", false),
    (RENUMBERED, "an id that disagrees with the lockfile", false),
    (LOCKFILE, "a malformed korek.lock", false),
    (
        PLUGIN_FAILED,
        "a plugin exited unsuccessfully or reported errors",
        false,
    ),
    (
        PLUGIN_PROTOCOL,
        "a plugin's response broke the protocol",
        false,
    ),
    (PLUGIN_NOT_FOUND, "no binary for a requested plugin", false),
    (
        INVALID_OPTION,
        "an unknown or malformed plugin option",
        false,
    ),
    (
        STALE_OUTPUT,
        "`--check`: a generated file is out of date",
        false,
    ),
];

pub const SYNTAX: &str = "korek::syntax";
pub const IO: &str = "korek::io";
pub const UNKNOWN_NODE: &str = "korek::unknown_node";
pub const UNKNOWN_PROPERTY: &str = "korek::unknown_property";
pub const UNKNOWN_TYPE: &str = "korek::unknown_type";
pub const UNKNOWN_RULE: &str = "korek::unknown_rule";
pub const DUPLICATE_NAME: &str = "korek::duplicate_name";
pub const NAME_CLASH: &str = "korek::name_clash";
pub const INVALID_NAME: &str = "korek::invalid_name";
pub const INVALID_TYPE: &str = "korek::invalid_type";
pub const INVALID_VALUE: &str = "korek::invalid_value";
pub const MISSING_VALUE: &str = "korek::missing_value";
pub const INVALID_DEFAULT: &str = "korek::invalid_default";
pub const INVALID_EXAMPLE: &str = "korek::invalid_example";
pub const INVALID_RULE: &str = "korek::invalid_rule";
pub const RULE_UNSUPPORTED: &str = "korek::rule_unsupported";
pub const INVALID_UNION: &str = "korek::invalid_union";
pub const INVALID_RPC: &str = "korek::invalid_rpc";
pub const CYCLE: &str = "korek::cycle";
pub const EMPTY: &str = "korek::empty";
pub const INVALID_ID: &str = "korek::invalid_id";
pub const DUPLICATE_ID: &str = "korek::duplicate_id";
pub const ID_TAKEN: &str = "korek::id_taken";
pub const RENUMBERED: &str = "korek::renumbered";
pub const LOCKFILE: &str = "korek::lockfile";
pub const PLUGIN_FAILED: &str = "korek::plugin_failed";
pub const PLUGIN_PROTOCOL: &str = "korek::plugin_protocol";
pub const PLUGIN_NOT_FOUND: &str = "korek::plugin_not_found";
pub const INVALID_OPTION: &str = "korek::invalid_option";
pub const STALE_OUTPUT: &str = "korek::stale_output";

/// The docs URL for a code.
pub fn url(code: &str) -> String {
    let anchor = code.split("::").last().unwrap_or(code);
    format!("https://github.com/talaagent/korek/blob/main/docs/errors.md#{anchor}")
}
