//! The spec's budgets, one constant per row of its budget table. Every limit the crate enforces
//! comes from here.

use crate::error::Error;

/// One budget from the spec: what it limits and how far.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Budget {
    /// The budget's name, as the spec's budget table words it.
    pub name: &'static str,
    /// The largest amount allowed.
    pub limit: usize,
}

impl Budget {
    /// The error for going past this budget.
    pub const fn exceeded(self) -> Error {
        Error::Budget {
            budget: self.name,
            limit: self.limit,
        }
    }
}

/// An inline envelope, in UTF-16 code units: the Telegram message limit.
pub const INLINE_ENVELOPE: Budget = Budget {
    name: "inline envelope",
    limit: 4096,
};

/// A template, in bytes: 256 KB.
pub const TEMPLATE_BYTES: Budget = Budget {
    name: "template bytes",
    limit: 256 * 1024,
};

/// State, in bytes: 64 KB.
pub const STATE_BYTES: Budget = Budget {
    name: "state bytes",
    limit: 64 * 1024,
};

/// Arrays and objects open at once in one JSON document.
pub const JSON_NESTING_DEPTH: Budget = Budget {
    name: "JSON nesting depth",
    limit: 64,
};

/// Node objects in a template, symbol shapes and fallback subtrees included.
pub const TEMPLATE_NODES: Budget = Budget {
    name: "template nodes",
    limit: 2000,
};

/// Nodes in one resolved tree.
pub const RESOLVED_NODES: Budget = Budget {
    name: "resolved nodes",
    limit: 10_000,
};

/// One resolved tree, in bytes as `twidget_resolve` writes it: 1 MB.
pub const RESOLVED_TREE: Budget = Budget {
    name: "resolved tree size",
    limit: 1024 * 1024,
};

/// Items one `repeat` expands into.
pub const ITEMS_PER_REPEAT: Budget = Budget {
    name: "items per repeat",
    limit: 1000,
};

/// Path drawing segments across a template, and again across a resolved tree.
pub const PATH_COMMANDS: Budget = Budget {
    name: "path commands across the tree",
    limit: 20_000,
};

/// One string, in bytes of decoded UTF-8: 8 KB.
pub const STRING_LENGTH: Budget = Budget {
    name: "string length",
    limit: 8 * 1024,
};

/// One expression's source, in bytes: 1 KB.
pub const EXPRESSION_LENGTH: Budget = Budget {
    name: "expression length",
    limit: 1024,
};

/// Nesting inside one expression.
pub const EXPRESSION_NESTING: Budget = Budget {
    name: "expression nesting",
    limit: 32,
};

/// Evaluation steps in one resolve.
pub const EVALUATION_FUEL: Budget = Budget {
    name: "evaluation fuel per resolve",
    limit: 200_000,
};

/// The largest view-box dimension; the smallest is [`VIEW_BOX_MIN`].
pub const VIEW_BOX_DIMENSIONS: Budget = Budget {
    name: "view-box dimensions",
    limit: 10_000,
};

/// The smallest view-box dimension.
pub const VIEW_BOX_MIN: usize = 1;
