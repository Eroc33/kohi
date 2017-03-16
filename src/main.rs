use std::path::{Path,PathBuf};
use std::process::{Command,ExitStatus};
use std::io;
use std::fs::{self, DirEntry};
use std::borrow::ToOwned;

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

fn source_root() -> Option<PathBuf>{
    const SEARCH_PATHS: &'static [&'static str] = &[
        "./src/main/java/",
        "./src/"
    ];
    
    SEARCH_PATHS.iter().map(|&s| Path::new(s).to_path_buf()).find(|p| dir_exists(p))
}

fn libs_root() -> Option<PathBuf>{
    const SEARCH_PATHS: &'static [&'static str] = &[
        "./libs/",
        "./deps/"
    ];

    SEARCH_PATHS.iter().map(|&s| Path::new(s).to_path_buf()).find(|p| dir_exists(p))
}

fn find_files_by_extension<P: AsRef<Path>>(path: P,extension: &str) -> Vec<PathBuf>{
    let mut found = vec![];
    visit_dirs(path,&mut |entry: &DirEntry|{
        let path = entry.path();
        if path.extension().is_some() && path.extension().unwrap() == extension{
            found.push(path);
        }
    }).expect("IO Error while finding file");
    found
}

fn source_files() -> Vec<PathBuf>{
    if let Some(dir) = source_root(){
        find_files_by_extension(dir,"java")
    }else{
        vec![]
    }
}

fn libs() -> Vec<PathBuf>{
    if let Some(dir) = libs_root(){
        find_files_by_extension(dir,"jar")
    }else{
        vec![]
    }
}

fn compile() -> Result<(),ExitStatus>{
    let source_path = source_root().expect("Couldn't locate source root");
    let classpath = std::env::join_paths(libs()).unwrap();

    let status = Command::new("javac")
                          .arg("-d")
                          .arg("dist")
                          .arg("-cp")
                          .arg(classpath)
                          .arg("-sourcepath")
                          .arg(source_path)
                          .args(source_files().as_slice())
                          .status()
                          .expect("Failed to fork javac process");
                          
    if status.success(){
        Ok(())
    }else{
        Err(status)
    }
}

fn find_class(class: &str) -> Option<String>{
    let dist_path = Path::new("./dist").to_path_buf();
    
    let class = class.to_lowercase();
    let mut class_name: Vec<_> = class.split(".").map(ToOwned::to_owned).collect();
    
    //kinda hacky, but this means we don't have to continuously strip off the dist prefix in the loop
    class_name.insert(0,"dist".to_owned());
    
    //also a hack to avoid removing the ".class" from the last component constantly
    let len = class_name.len();
    class_name[len-1].push_str(".class");
        
    let path = find_in_dirs(dist_path.as_path(),&mut |entry|{
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
        let mut path = path.strip_prefix(dist_path.as_path()).expect("Found something outside of ./dist/ in ./dist/ ???").to_path_buf();
        path.set_extension("");
        
        let path: Vec<_> = path.iter().flat_map(|s| s.to_str()).map(|s| s.to_owned()).collect();
        let path = path.join(".");
        
        let path_as_class = path.trim_left_matches(".");
        Some(path_as_class.to_owned())
    }else{
        None
    }
}


fn run(run_class: String) -> Result<(),ExitStatus>{
    let dist_path = Path::new("./dist/").to_path_buf();
    
    //blah.class -> blah
    let run_class = run_class.trim_right_matches(".class");

    let classpath = std::env::join_paths(std::iter::once(dist_path).chain(libs())).unwrap();
    
    let main = find_class(run_class).expect("Couldn't locate the class to run");

    let status = Command::new("java")
                      .arg("-cp")
                      .arg(classpath)
                      .arg(main)
                      .status()
                      .expect("Failed to fork java process");
                      
    if status.success(){
        Ok(())
    }else{
        Err(status)
    }
}

fn main() {
    let run_class = std::env::args().nth(1).unwrap_or("--no-run".to_owned());
    
    println!("Compiling");
    let compile_result = compile();
    
    let status = if run_class == "--no-run"{
        println!("No class passed, will not run");
        compile_result
    }else{
        compile_result.and_then(move |_|{
            run(run_class)
        })
    };
    match status{
        Err(e) => println!("Run failed with {}.",e),
        Ok(()) => println!("Done."),
    }
}
