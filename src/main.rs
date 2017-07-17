extern crate kohi;

use kohi::Kohi;

fn main() {
    let run_class = std::env::args().nth(1).unwrap_or("--no-run".to_owned());
    
    let kohi = match Kohi::new(){
        Ok(kohi) => kohi,
        Err(e) => {
            println!("Error: {}",e);
            std::process::exit(1);
        }
    };
    
    println!("Compiling");
    
    let compile_result = kohi.compile();
    
    let status = if run_class == "--no-run"{
        println!("No class passed, will not run");
        compile_result
    }else{
        compile_result.and_then(move |kohi|{
            kohi.run(run_class)
        })
    };
    match status{
        Err(e) => {
            println!("Run failed with \"{}\".",e);
            std::process::exit(1);
        }
        Ok(_) => println!("Done."),
    }
}
