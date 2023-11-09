use std::{
    collections::{HashMap, HashSet},
    sync::Mutex,
};

use once_cell::sync::Lazy;
use tree_sitter::{Language, Parser, Query, QueryCursor};

extern "C" {
    fn tree_sitter_vhdl() -> Language;
}

static VHDL_TREE_SITTER_LANGUAGE: Lazy<Language> = Lazy::new(|| unsafe { tree_sitter_vhdl() });

static VHDL_TREE_SITTER: Lazy<Mutex<Parser>> = Lazy::new(|| {
    let mut parser = Parser::new();
    parser.set_language(*VHDL_TREE_SITTER_LANGUAGE).unwrap();
    Mutex::new(parser)
});

fn get_components_of<P: AsRef<std::path::Path>>(
    path: P,
) -> Result<Vec<String>, Box<dyn std::error::Error>> {
    let code_src = std::fs::read_to_string(path)?;
    let mut parser = VHDL_TREE_SITTER.lock()?;
    let tree = parser.parse(&code_src, None).unwrap();

    let query = Query::new(
        *VHDL_TREE_SITTER_LANGUAGE,
        "(component_declaration name: (identifier) @name)",
    )
    .unwrap();

    let mut qc = QueryCursor::new();

    Ok(qc
        .matches(&query, tree.root_node(), code_src.as_bytes())
        .flat_map(|node| node.nodes_for_capture_index(0).next())
        .map(|t| t.byte_range())
        .map(|range| &code_src[range])
        .map(ToOwned::to_owned)
        .collect())
}

pub fn generate_sources_for<P: AsRef<std::path::Path>>(path: P) -> HashSet<std::path::PathBuf> {
    fn generate_sources_inner(
        path: &std::path::Path,
        set: &mut HashMap<std::path::PathBuf, Vec<std::path::PathBuf>>,
    ) {
        if set.contains_key(path) {
            return;
        }

        let Ok(components) = get_components_of(path) else {
            return;
        };

        let paths = components
            .iter()
            .map(|comp| path.with_file_name(comp).with_extension("vhd"))
            .filter(|path| path.exists());

        set.insert(path.to_owned(), paths.clone().collect());
        // set all the direct dependencies of the current path

        for path in paths {
            generate_sources_inner(&path, set);
        }
    }

    let mut map = HashMap::new();
    generate_sources_inner(path.as_ref(), &mut map);

    let mut set = HashSet::new();
    for (k, v) in map {
        set.insert(k);
        set.extend(v);
    }
    set
}
