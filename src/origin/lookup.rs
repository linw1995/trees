use snafu::Snafu;

use crate::storage::OriginRepositoryRow;

pub fn find_url(
    rows: &[OriginRepositoryRow],
    url: &str,
) -> Result<Option<OriginRepositoryRow>, LookupError> {
    let mut matches = Vec::new();
    for row in rows {
        let Ok(info) = crate::git::inspect_repository(&row.source_path) else {
            continue;
        };
        if info.common_dir != row.repository_identity || info.root != row.source_path {
            continue;
        }
        if crate::git::origin_remote_urls(&row.source_path)?
            .iter()
            .any(|remote| remote == url)
        {
            matches.push(row);
        }
    }
    if matches.len() > 1 {
        return AmbiguousSnafu {
            paths: matches
                .iter()
                .map(|row| row.source_path.to_string())
                .collect::<Vec<_>>()
                .join(", "),
        }
        .fail();
    }
    Ok(matches.first().map(|row| (*row).clone()))
}

#[derive(Debug, Snafu)]
pub enum LookupError {
    #[snafu(transparent)]
    Git { source: crate::git::GitError },
    #[snafu(display("repository URL matches multiple origins; use an explicit path: {paths}"))]
    Ambiguous { paths: String },
}
