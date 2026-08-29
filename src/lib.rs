//! MageHat: a tiny, deterministic compiler for plain HTML sites.
//!
//! The library exposes the build so integration tests can compile sites in
//! memory; the `magehat` binary is a thin command-line wrapper around it.

pub mod assets;
pub mod build;
pub mod check;
pub mod cli;
pub mod components;
pub mod config;
pub mod content;
pub mod dev;
pub mod errors;
pub mod expr;
pub mod fonts;
pub mod frontmatter;
pub mod htmltree;
pub mod icons;
pub mod images;
pub mod init;
pub mod inspect;
pub mod jsonld;
pub mod library;
pub mod lint;
pub mod markdown;
pub mod minify;
pub mod new;
pub mod pages;
pub mod render;
pub mod seo;
pub mod values;
