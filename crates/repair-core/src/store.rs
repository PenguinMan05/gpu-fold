use crate::types::*;
use anyhow::{Context, Result};
use rusqlite::{params, Connection};
use std::path::{Path, PathBuf};

#[derive(Clone)]
pub struct Store { pub root: PathBuf }
impl Store {
    pub fn open(root: impl AsRef<Path>) -> Result<Self> {
        std::fs::create_dir_all(root.as_ref())?;
        #[cfg(unix)] { use std::os::unix::fs::PermissionsExt; std::fs::set_permissions(root.as_ref(),std::fs::Permissions::from_mode(0o700))?; }
        let s=Self{root:root.as_ref().to_path_buf()};
        let c=s.connection()?;
        c.execute_batch("PRAGMA journal_mode=WAL; CREATE TABLE IF NOT EXISTS runs(id TEXT PRIMARY KEY, created INTEGER NOT NULL, updated INTEGER NOT NULL, data TEXT NOT NULL); CREATE TABLE IF NOT EXISTS settings(id INTEGER PRIMARY KEY, data TEXT NOT NULL);")?;
        Ok(s)
    }
    fn connection(&self)->Result<Connection>{let c=Connection::open(self.root.join("history.sqlite"))?;c.busy_timeout(std::time::Duration::from_secs(5))?;Ok(c)}
    pub fn save(&self,run:&Run)->Result<()> { self.connection()?.execute("INSERT INTO runs(id,created,updated,data) VALUES(?1,?2,?3,?4) ON CONFLICT(id) DO UPDATE SET updated=excluded.updated,data=excluded.data",params![run.id,run.created as i64,run.updated as i64,serde_json::to_string(run)?])?;Ok(()) }
    pub fn get(&self,id:&str)->Result<Run>{let raw:String=self.connection()?.query_row("SELECT data FROM runs WHERE id=?1",[id],|r|r.get(0)).context("Run not found")?;Ok(serde_json::from_str(&raw)?)}
    pub fn list(&self)->Result<Vec<Run>>{let c=self.connection()?;let mut s=c.prepare("SELECT data FROM runs ORDER BY updated DESC LIMIT 200")?;let rows=s.query_map([],|r|r.get::<_,String>(0))?;let mut runs=vec![];for row in rows {runs.push(serde_json::from_str(&row?)?);}Ok(runs)}
    pub fn settings(&self)->Result<Settings>{let c=self.connection()?;let raw=c.query_row("SELECT data FROM settings WHERE id=1",[],|r|r.get::<_,String>(0));match raw{Ok(s)=>Ok(serde_json::from_str(&s)?),Err(rusqlite::Error::QueryReturnedNoRows)=>Ok(Settings::default()),Err(e)=>Err(e.into())}}
    pub fn save_settings(&self,s:&Settings)->Result<()>{self.connection()?.execute("INSERT INTO settings(id,data) VALUES(1,?1) ON CONFLICT(id) DO UPDATE SET data=excluded.data",[serde_json::to_string(s)?])?;Ok(())}
    pub fn export(&self,run:&Run,path:&Path)->Result<()>{std::fs::write(path,serde_json::to_vec_pretty(run)?)?;Ok(())}
}
pub fn default_root()->PathBuf{std::env::var_os("XDG_DATA_HOME").map(PathBuf::from).unwrap_or_else(||std::env::var_os("HOME").map(PathBuf::from).unwrap_or_else(||PathBuf::from(".")).join(".local/share")).join("rust-repair")}
