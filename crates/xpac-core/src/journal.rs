use crate::{execution::ExecutionRecord, Error, Plan};
use serde::{Deserialize, Serialize};
use std::{
    fs,
    io::{Read, Write},
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Transaction {
    pub schema_version: u32,
    pub id: String,
    pub started_unix_ms: u128,
    pub plan: Plan,
    /// None means interrupted/unknown, including a transaction currently running.
    pub result: Option<ExecutionRecord>,
}
#[derive(Debug, Default, Serialize)]
pub struct JournalStatus {
    pub total: usize,
    pub pending_or_unknown: usize,
    pub successful: usize,
    pub failed: usize,
}
pub struct Journal {
    directory: PathBuf,
}
impl Journal {
    pub fn new(directory: impl Into<PathBuf>) -> Self {
        Self {
            directory: directory.into(),
        }
    }
    fn io(&self, source: std::io::Error) -> Error {
        Error::Io {
            program: "transaction journal".into(),
            source,
        }
    }
    fn path(&self, id: &str) -> Result<PathBuf, Error> {
        if id.is_empty() || !id.bytes().all(|b| b.is_ascii_digit() || b == b'-') {
            return Err(Error::InvalidInput("invalid transaction identifier".into()));
        }
        Ok(self.directory.join(format!("{id}.json")))
    }
    fn prepare(&self) -> Result<(), Error> {
        let mut builder = fs::DirBuilder::new();
        builder.recursive(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::DirBuilderExt;
            builder.mode(0o700);
        }
        builder.create(&self.directory).map_err(|e| self.io(e))?;
        let metadata = fs::symlink_metadata(&self.directory).map_err(|e| self.io(e))?;
        if !metadata.is_dir() || metadata.file_type().is_symlink() {
            return Err(Error::Safety(
                "journal directory must be a real directory".into(),
            ));
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::MetadataExt;
            // SAFETY: geteuid returns a scalar process identifier.
            if metadata.uid() != unsafe { libc::geteuid() } {
                return Err(Error::Safety(
                    "journal directory belongs to another user".into(),
                ));
            }
            if metadata.mode() & 0o077 != 0 {
                return Err(Error::Safety("journal directory must be private; choose a dedicated directory with mode 0700".into()));
            }
        }
        Ok(())
    }
    fn write(&self, transaction: &Transaction, create: bool) -> Result<(), Error> {
        let target = self.path(&transaction.id)?;
        let mut temp = tempfile::NamedTempFile::new_in(&self.directory).map_err(|e| self.io(e))?;
        serde_json::to_writer(&mut temp, transaction)
            .map_err(|_| Error::Parse("cannot serialize transaction".into()))?;
        if temp.as_file().metadata().map_err(|e| self.io(e))?.len() > 16 * 1024 * 1024 {
            return Err(Error::Safety(
                "transaction journal entry exceeds 16 MiB; native execution refused".into(),
            ));
        }
        temp.flush().map_err(|e| self.io(e))?;
        temp.as_file().sync_all().map_err(|e| self.io(e))?;
        if create {
            temp.persist_noclobber(target)
                .map_err(|e| self.io(e.error))?;
        } else {
            temp.persist(target).map_err(|e| self.io(e.error))?;
        }
        #[cfg(unix)]
        fs::File::open(&self.directory)
            .and_then(|dir| dir.sync_all())
            .map_err(|e| self.io(e))?;
        Ok(())
    }
    pub fn begin(&self, plan: &Plan) -> Result<String, Error> {
        self.prepare()?;
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|_| Error::Safety("system clock predates Unix epoch".into()))?;
        let id = format!("{}-{}", now.as_nanos(), std::process::id());
        self.write(
            &Transaction {
                schema_version: 1,
                id: id.clone(),
                started_unix_ms: now.as_millis(),
                plan: plan.clone(),
                result: None,
            },
            true,
        )?;
        Ok(id)
    }
    fn read(&self, path: &Path) -> Result<Transaction, Error> {
        let metadata = fs::symlink_metadata(path).map_err(|e| self.io(e))?;
        if !metadata.is_file() || metadata.file_type().is_symlink() {
            return Err(Error::Safety("journal entry must be a regular file".into()));
        }
        let mut bytes = vec![];
        fs::File::open(path)
            .map_err(|e| self.io(e))?
            .take(16 * 1024 * 1024 + 1)
            .read_to_end(&mut bytes)
            .map_err(|e| self.io(e))?;
        if bytes.len() > 16 * 1024 * 1024 {
            return Err(Error::Parse("journal entry exceeds 16 MiB".into()));
        }
        let transaction: Transaction = serde_json::from_slice(&bytes)
            .map_err(|_| Error::Parse("invalid transaction journal entry".into()))?;
        if transaction.schema_version != 1 {
            return Err(Error::Unsupported(
                "transaction journal schema version".into(),
            ));
        }
        if self.path(&transaction.id)?.file_name() != path.file_name() {
            return Err(Error::Parse(
                "journal identifier does not match filename".into(),
            ));
        }
        Ok(transaction)
    }
    pub fn finish(&self, id: &str, record: &ExecutionRecord) -> Result<(), Error> {
        let mut transaction = self.read(&self.path(id)?)?;
        if transaction.result.is_some() {
            return Err(Error::Safety("transaction is already finalized".into()));
        }
        transaction.result = Some(record.clone());
        self.write(&transaction, false)
    }
    fn entries(&self) -> Result<Vec<PathBuf>, Error> {
        let entries = match fs::read_dir(&self.directory) {
            Ok(entries) => entries,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(vec![]),
            Err(e) => return Err(self.io(e)),
        };
        let mut paths = vec![];
        for entry in entries {
            let path = entry.map_err(|e| self.io(e))?.path();
            if path
                .extension()
                .is_some_and(|extension| extension == "json")
            {
                paths.push(path);
            }
        }
        Ok(paths)
    }
    /// Reading missing history never creates a directory or changes a record.
    pub fn history(&self) -> Result<Vec<Transaction>, Error> {
        let mut result = self
            .entries()?
            .iter()
            .map(|path| self.read(path))
            .collect::<Result<Vec<_>, _>>()?;
        result.sort_by(|a, b| {
            b.started_unix_ms
                .cmp(&a.started_unix_ms)
                .then_with(|| b.id.cmp(&a.id))
        });
        Ok(result)
    }
    /// Select filenames before decoding large plans. IDs encode creation nanos;
    /// older records are not deserialized just to display one bounded page.
    pub fn history_page(&self, offset: usize, limit: usize) -> Result<Vec<Transaction>, Error> {
        if !(1..=1000).contains(&limit) {
            return Err(Error::InvalidInput(
                "history page limit must be 1–1000".into(),
            ));
        }
        let mut paths = self
            .entries()?
            .into_iter()
            .map(|path| {
                let stem = path
                    .file_stem()
                    .and_then(|s| s.to_str())
                    .ok_or_else(|| Error::Parse("invalid journal filename".into()))?;
                let (nanos, pid) = stem.split_once('-').ok_or_else(|| {
                    Error::Parse("journal filename lacks creation timestamp".into())
                })?;
                let key = (
                    nanos
                        .parse::<u128>()
                        .map_err(|_| Error::Parse("invalid journal creation timestamp".into()))?,
                    pid.parse::<u32>()
                        .map_err(|_| Error::Parse("invalid journal process identifier".into()))?,
                );
                Ok((key, path))
            })
            .collect::<Result<Vec<_>, Error>>()?;
        paths.sort_by_key(|entry| std::cmp::Reverse(entry.0));
        paths
            .into_iter()
            .skip(offset)
            .take(limit)
            .map(|(_, path)| self.read(&path))
            .collect()
    }
    /// Decode/drop one bounded record at a time instead of retaining every plan.
    pub fn status(&self) -> Result<JournalStatus, Error> {
        let mut status = JournalStatus::default();
        for path in self.entries()? {
            let transaction = self.read(&path)?;
            status.total += 1;
            match transaction.result {
                None => status.pending_or_unknown += 1,
                Some(record) if record.success => status.successful += 1,
                Some(_) => status.failed += 1,
            }
        }
        Ok(status)
    }
}
