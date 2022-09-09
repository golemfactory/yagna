use crate::error::Error;
use globset::{GlobBuilder, GlobSet, GlobSetBuilder};
use std::ops::Not;
use std::path::{Path, PathBuf, StripPrefixError};
use walkdir::WalkDir;
use ya_client_model::activity::{FileSet, SetEntry, SetObject, TransferArgs};
use ya_utils_path::normalize_path;

pub trait PathTraverse {
    fn traverse<P: AsRef<Path>>(&self, path: P)
        -> Result<Box<dyn Iterator<Item = PathBuf>>, Error>;
}

impl PathTraverse for TransferArgs {
    fn traverse<P: AsRef<Path>>(
        &self,
        path: P,
    ) -> Result<Box<dyn Iterator<Item = PathBuf>>, Error> {
        let path = normalize_path(path)?;
        let mut walk = WalkDir::new(&path).min_depth(1);
        if let Some(depth) = self.depth {
            walk = walk.max_depth(depth + 1);
        }

        let it = walk
            .into_iter()
            .filter_map(Result::ok)
            .filter_map(|e| normalize_path(e.path()).ok())
            .filter_paths(path, self.fileset.as_ref())?;
        Ok(Box::new(it))
    }
}

pub trait PathFilter<P, B>
where
    P: AsRef<Path> + 'static,
    B: PatternBuilder,
{
    fn filter_paths<R>(
        self,
        path: R,
        pattern_builder: Option<&B>,
    ) -> Result<Box<dyn Iterator<Item = P>>, Error>
    where
        R: AsRef<Path>;
}

impl<P, I, B> PathFilter<P, B> for I
where
    P: AsRef<Path> + 'static,
    I: Iterator<Item = P> + 'static,
    B: PatternBuilder,
{
    fn filter_paths<R>(
        self,
        path: R,
        pattern_builder: Option<&B>,
    ) -> Result<Box<dyn Iterator<Item = P>>, Error>
    where
        R: AsRef<Path>,
    {
        let path = normalize_path(path)?;
        let (incl, excl) = match pattern_builder {
            Some(b) => b.build()?,
            _ => FileSet::Pattern(SetEntry::Single("**/*".into())).build()?,
        };

        let it = self
            .map(move |p| {
                let relative = p.as_ref().strip_prefix(&path)?.to_owned();
                Ok::<_, StripPrefixError>((p, relative))
            })
            .filter_map(Result::ok)
            .filter(move |(_, r)| excl.is_match(r).not())
            .filter(move |(_, r)| incl.is_match(r))
            .map(|(p, _)| p);
        Ok(Box::new(it))
    }
}

pub trait PatternBuilder {
    fn add_patterns<'l>(&'l self, incl: &mut Vec<&'l str>, excl: &mut Vec<&'l str>);

    fn build(&self) -> Result<(GlobSet, GlobSet), Error> {
        let mut incl = Vec::new();
        let mut excl = Vec::new();
        let mut incl_builder = GlobSetBuilder::new();
        let mut excl_builder = GlobSetBuilder::new();

        self.add_patterns(&mut incl, &mut excl);

        incl.into_iter().try_for_each(|p| {
            incl_builder.add(GlobBuilder::new(p).literal_separator(true).build()?);
            Ok::<_, Error>(())
        })?;
        excl.into_iter().try_for_each(|p| {
            excl_builder.add(GlobBuilder::new(p).literal_separator(true).build()?);
            Ok::<_, Error>(())
        })?;

        Ok((incl_builder.build()?, excl_builder.build()?))
    }
}

impl<T: PatternBuilder> PatternBuilder for Vec<T> {
    fn add_patterns<'l>(&'l self, incl: &mut Vec<&'l str>, excl: &mut Vec<&'l str>) {
        self.iter().for_each(|p| (*p).add_patterns(incl, excl));
    }
}

impl PatternBuilder for FileSet {
    fn add_patterns<'l>(&'l self, incl: &mut Vec<&'l str>, excl: &mut Vec<&'l str>) {
        match self {
            FileSet::Pattern(e) => (*e).add_pattern(incl),
            FileSet::Object(e) => (*e).add_patterns(incl, excl),
        }
    }
}

impl<T: PatternBuilder> PatternBuilder for SetEntry<T> {
    fn add_patterns<'l>(&'l self, incl: &mut Vec<&'l str>, excl: &mut Vec<&'l str>) {
        match self {
            SetEntry::Single(s) => (*s).add_patterns(incl, excl),
            SetEntry::Multiple(m) => (*m).add_patterns(incl, excl),
        }
    }
}

impl PatternBuilder for SetObject {
    fn add_patterns<'l>(&'l self, incl: &mut Vec<&'l str>, excl: &mut Vec<&'l str>) {
        if let Some(includes) = &self.includes {
            includes.add_pattern(incl);
        }
        if let Some(excludes) = &self.excludes {
            excludes.add_pattern(excl);
        }
    }
}

trait PatternProvider {
    fn add_pattern<'l>(&'l self, vec: &mut Vec<&'l str>);
}

impl PatternProvider for String {
    fn add_pattern<'l>(&'l self, vec: &mut Vec<&'l str>) {
        vec.push(self.as_str());
    }
}

impl<T: PatternProvider> PatternProvider for Vec<T> {
    fn add_pattern<'l>(&'l self, vec: &mut Vec<&'l str>) {
        self.iter().for_each(|t| t.add_pattern(vec));
    }
}

impl<T: PatternProvider> PatternProvider for SetEntry<T> {
    fn add_pattern<'l>(&'l self, vec: &mut Vec<&'l str>) {
        match self {
            SetEntry::Single(s) => s.add_pattern(vec),
            SetEntry::Multiple(m) => (*m).add_pattern(vec),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs::create_dir_all;
    use std::mem;
    use std::path::Path;

    struct Builder {
        format: Option<String>,
        depth: Option<usize>,
        includes: Option<SetEntry<String>>,
        excludes: Option<SetEntry<String>>,
    }

    impl Builder {
        pub fn new() -> Self {
            Builder {
                format: None,
                depth: None,
                includes: None,
                excludes: None,
            }
        }

        pub fn depth(mut self, depth: usize) -> Self {
            self.depth = Some(depth);
            self
        }

        pub fn include<S: ToString>(mut self, glob: S) -> Self {
            Self::include_in(&mut self.includes, glob);
            self
        }

        pub fn exclude<S: ToString>(mut self, glob: S) -> Self {
            Self::include_in(&mut self.excludes, glob);
            self
        }

        pub fn build(self) -> TransferArgs {
            let format = self.format;
            let depth = self.depth;
            let fileset = if self.includes.is_some() || self.excludes.is_some() {
                Some(FileSet::Object(SetEntry::Single(SetObject {
                    desc: None,
                    includes: self.includes,
                    excludes: self.excludes,
                })))
            } else {
                None
            };

            TransferArgs {
                format,
                depth,
                fileset,
            }
        }

        fn include_in<S: ToString>(set: &mut Option<SetEntry<String>>, glob: S) {
            let glob = glob.to_string();
            let includes = match set.take() {
                Some(entry) => match entry {
                    SetEntry::Single(e) => Some(SetEntry::Multiple(vec![e, glob])),
                    SetEntry::Multiple(mut v) => {
                        v.push(glob);
                        Some(SetEntry::Multiple(v))
                    }
                },
                _ => Some(SetEntry::Multiple(vec![glob])),
            };
            let _ = mem::replace(set, includes);
        }
    }

    fn create_files(dir: &Path) {
        let files = vec![
            dir.join("root file"),
            dir.join("root-file.txt"),
            dir.join("root file.txt"),
            dir.join("root filx.txt"),
            dir.join("work").join("first file.bin"),
            dir.join("work").join("second file.txt"),
            dir.join("work").join("d1_d1").join("file x.txt"),
            dir.join("work").join("d1_d1").join("unknown.bin"),
            dir.join("free").join("other filx.bin"),
        ];

        files.iter().for_each(|f| {
            create_dir_all(f.parent().unwrap()).unwrap();
            std::fs::File::create(f).unwrap();
        });
    }

    #[test]
    fn asterisk() -> anyhow::Result<()> {
        let dir = tempdir::TempDir::new("test-glob")?;
        create_files(dir.path());

        let items_len = Builder::new()
            .include("*.txt")
            .build()
            .traverse(dir.path())?
            .count();

        assert_eq!(items_len, 3);

        let items_len = Builder::new()
            .include("**/*.txt")
            .build()
            .traverse(dir.path())?
            .count();

        assert_eq!(items_len, 5);

        Ok(())
    }

    #[test]
    fn all() -> anyhow::Result<()> {
        let dir = tempdir::TempDir::new("test-glob")?;
        create_files(dir.path());

        let items_len = Builder::new().build().traverse(dir.path())?.count();

        assert_eq!(items_len, 3 + 9);
        Ok(())
    }

    #[test]
    fn include() -> anyhow::Result<()> {
        let dir = tempdir::TempDir::new("test-glob").unwrap();
        create_files(dir.path());

        let names = Builder::new()
            .include("**/* fil?.txt")
            .build()
            .traverse(dir.path())?
            .map(|d| d.file_name().unwrap().to_string_lossy().to_string())
            .collect::<Vec<_>>();
        let expected = vec!["root file.txt", "root filx.txt", "second file.txt"]
            .into_iter()
            .map(|s| s.to_string())
            .collect::<Vec<_>>();

        assert_eq!(names.len(), expected.len());
        assert!(expected.iter().all(|e| names.contains(e)));
        Ok(())
    }

    #[test]
    fn exclude() -> anyhow::Result<()> {
        let dir = tempdir::TempDir::new("test-glob").unwrap();
        create_files(dir.path());

        let names = Builder::new()
            .include("**/*file.*")
            .exclude("**/*.txt")
            .build()
            .traverse(dir.path())?
            .map(|d| d.file_name().unwrap().to_string_lossy().to_string())
            .collect::<Vec<_>>();
        let expected = vec!["first file.bin".to_string()];

        assert_eq!(names.len(), expected.len());
        assert!(expected.iter().all(|e| names.contains(e)));
        Ok(())
    }

    #[test]
    fn depth() -> anyhow::Result<()> {
        let dir = tempdir::TempDir::new("test-glob").unwrap();
        create_files(dir.path());

        let entries_len = Builder::new()
            .include("**/*.txt")
            .depth(0)
            .build()
            .traverse(dir.path())?
            .count();
        assert_eq!(entries_len, 3_usize);

        let entries_len = Builder::new()
            .include("**/*.txt")
            .depth(1)
            .build()
            .traverse(dir.path())?
            .count();
        assert_eq!(entries_len, 4_usize);

        let entries_len = Builder::new()
            .include("**/*.txt")
            .depth(2)
            .build()
            .traverse(dir.path())?
            .count();
        assert_eq!(entries_len, 5_usize);

        Ok(())
    }
}
