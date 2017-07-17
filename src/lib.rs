#[macro_use]
extern crate error_chain;

use std::path::{Path,PathBuf};
use std::process::{Command,ExitStatus};
use std::io;
use std::fs::{self, DirEntry};
use std::borrow::ToOwned;

error_chain!{
    foreign_links {
        Io(::std::io::Error);
    }
    errors{
        MissingSourceDir{
            description("Couldn't locate source directory")
            display("Couldn't locate your source directory")
        }
        ForkFailure(command: &'static str){
            description("Failed to fork command")
            display("Failed to fork command {}",command)
        }
        ExitFailure(command: &'static str, status: ExitStatus){
            description("Command exited with failure status")
            display("Command {} exited with status: {}",command,status)
        }
        DistNotDir(dist_path: PathBuf){
            description("The dist path did not point to a directory")
            display("The path {:?} must not point to an existing non-directory file",dist_path)
        }
        NoSources(source_path: PathBuf){
            description("The located source path contains no source files")
            display("The located source path ({:?}) contained no `.java` files",source_path)
        }
    }
}

fn visit_dirs<P: AsRef<Path>,F: FnMut(&DirEntry)>(dir: P, cb: &mut F) -> io::Result<()> {
    let dir = dir.as_ref();
    if dir.is_dir() {
        for entry in try!(fs::read_dir(dir)) {
            let entry = try!(entry);
            let path = entry.path();
            if path.is_dir() {
                try!(visit_dirs(&path, cb));
            } else {
                cb(&entry);
            }
        }
    }
    Ok(())
}

fn find_in_dirs<P: AsRef<Path>,F:FnMut(&DirEntry) -> Option<T>,T>(dir: P, cb: &mut F) -> io::Result<Option<T>> {
    let dir = dir.as_ref();
    if dir.is_dir() {
        for entry in try!(fs::read_dir(dir)) {
            let entry = try!(entry);
            let path = entry.path();
            if path.is_dir() {
                match try!(find_in_dirs(&path, cb)){
                    Some(t) => return Ok(Some(t)),
                    None => continue
                }
            } else {
                match cb(&entry){
                    Some(t) => return Ok(Some(t)),
                    None => continue,
                }
            }
        }
    }
    Ok(None)
}

fn dir_exists<P: AsRef<Path>>(path: P) -> bool{
    let path = path.as_ref();
    path.exists() && path.is_dir()
}

fn source_root() -> Result<PathBuf>{
    const SEARCH_PATHS: &'static [&'static str] = &[
        "./src/main/java/",
        "./src/java/",
        "./src/"
    ];
    
    SEARCH_PATHS.iter().map(|&s| Path::new(s).to_path_buf()).find(|p| dir_exists(p)).ok_or(ErrorKind::MissingSourceDir.into())
}

fn libs_root() -> Option<PathBuf>{
    const SEARCH_PATHS: &'static [&'static str] = &[
        "./libs/",
        "./deps/"
    ];

    SEARCH_PATHS.iter().map(|&s| Path::new(s).to_path_buf()).find(|p| dir_exists(p))
}

fn find_files_by_extension<P: AsRef<Path>>(root_path: P,extension: &str, strip_root_prefix: bool) -> Result<Vec<PathBuf>>{
    let mut found = vec![];
    visit_dirs(root_path.as_ref(),&mut |entry: &DirEntry|{
        let path = entry.path();
        if path.extension().is_some() && path.extension().unwrap() == extension{
            let path = if strip_root_prefix {
                path.strip_prefix(&root_path).expect("Found something outside of $root_path in $root_path ???").to_path_buf()
            }else{
                path
            };
            found.push(path);
        }
    })?;
    Ok(found)
}

fn source_files<P: AsRef<Path>>(source_root: P) -> Result<Vec<PathBuf>>{
    find_files_by_extension(source_root,"java",false)
}

fn class_files() -> Result<Vec<PathBuf>>{
    find_files_by_extension("dist","class",true)
}

fn libs() -> Result<Vec<PathBuf>>{
    if let Some(dir) = libs_root(){
        find_files_by_extension(dir,"jar",false)
    }else{
        Ok(vec![])
    }
}

pub struct Kohi{
    dist_path: PathBuf,
    libs: Vec<PathBuf>,
    source_path: PathBuf,
    source_files: Vec<PathBuf>
}

impl Kohi{
    pub fn new() -> Result<Self>{
        let source_root = source_root()?;
        let source_files = source_files(&source_root)?;
        if source_files.len() == 0 {
            bail!(ErrorKind::NoSources(source_root));
        }
        Ok(Kohi{
            dist_path: Path::new("./dist/").to_path_buf(),
            libs: libs()?,
            source_path: source_root,
            source_files: source_files,
        })
    }
    
    pub fn compile<I: Into<Option<String>>>(self,target_version: I) -> Result<Self>{
        let classpath = std::env::join_paths(&self.libs).unwrap();
        
        if !self.dist_path.exists(){
            std::fs::create_dir_all(&self.dist_path)?;
        }else{
            if !self.dist_path.is_dir(){
                bail!(ErrorKind::DistNotDir(self.dist_path));
            }
        }
        
        let mut cmd = Command::new("javac");
        
        if let Some(target_version) = target_version.into(){
            cmd.arg("-target").arg(target_version);
        }

        let status = cmd
            .arg("-d")
            .arg(&self.dist_path)
            .arg("-cp")
            .arg(classpath)
            .arg("-sourcepath")
            .arg(&self.source_path)
            .args(self.source_files.as_slice())
            .status()
            .chain_err(|| ErrorKind::ForkFailure("javac"))?;
                              
        if status.success(){
            Ok(self)
        }else{
            bail!(ErrorKind::ExitFailure("javac",status))
        }
    }
    
    pub fn run(self, run_class: String) -> Result<Self>{
        //blah.class -> blah
        let run_class = run_class.trim_right_matches(".class");

        let classpath = std::env::join_paths(std::iter::once(&self.dist_path).chain(&self.libs)).unwrap();
        
        let main = self.find_class(run_class).expect("Couldn't locate the class to run");

        let status = Command::new("java")
                          .arg("-cp")
                          .arg(classpath)
                          .arg(main)
                          .status()
                          .chain_err(|| ErrorKind::ForkFailure("java"))?;
                          
        if status.success(){
            Ok(self)
        }else{
            bail!(ErrorKind::ExitFailure("java",status))
        }
    }

    pub fn package(self, jar_name: &str, entry_point: Option<&str>) -> Result<Self>{
        let mut cmd = Command::new("jar");
        if let Some(entry_point) = entry_point{
            cmd.arg("-cfe")
                .arg(jar_name)
                .arg(entry_point);
        }else{
            cmd.arg("-cf")
                .arg(jar_name);
        }

        let status = cmd
            .arg("-C")
            .arg("dist")
            .args(class_files()?.as_slice())
            .status()
            .chain_err(|| ErrorKind::ForkFailure("jar"))?;
            
        if status.success(){
            Ok(self)
        }else{
            bail!(ErrorKind::ExitFailure("jar",status))
        }
    }
    
    fn find_class(&self,class: &str) -> Option<String>{
        
        let class = class.to_lowercase();
        let mut class_name: Vec<_> = class.split(".").map(ToOwned::to_owned).collect();
        
        //kinda hacky, but this means we don't have to continuously strip off the dist prefix in the loop
        class_name.insert(0,"dist".to_owned());
        
        //also a hack to avoid removing the ".class" from the last component constantly
        let len = class_name.len();
        class_name[len-1].push_str(".class");
            
        let path = find_in_dirs(self.dist_path.as_path(),&mut |entry|{
            let path = entry.path();
            {
                let components = path.components().filter_map(|comp|{
                    use std::path::Component;
                    match comp{
                        Component::Normal(str) => Some(str),
                        _ => None
                    }
                });
                
                for (a,b) in components.zip(class_name.iter()){
                    if let Some(a) = a.to_str().map(str::to_lowercase){
                        if &a == b{
                            continue
                        }else{
                            return None
                        }
                    }else{
                        return None
                    }
                }
            }
            Some(path)
        }).expect("IO Error while finding class file");
        
        if let Some(path) = path{
            let mut path = path.strip_prefix(self.dist_path.as_path()).expect("Found something outside of ./dist/ in ./dist/ ???").to_path_buf();
            path.set_extension("");
            
            let path: Vec<_> = path.iter().flat_map(|s| s.to_str()).map(|s| s.to_owned()).collect();
            let path = path.join(".");
            
            let path_as_class = path.trim_left_matches(".");
            Some(path_as_class.to_owned())
        }else{
            None
        }
    }
}