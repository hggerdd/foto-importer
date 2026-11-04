fn main(){
    let args=std::env::args().collect::<Vec<_>>();
    let Some(index)=args.iter().position(|a|a=="-@")else{std::process::exit(2);};
    let paths=std::fs::read_to_string(&args[index+1]).expect("argument file");
    let values=paths.lines().filter(|p|!p.is_empty()).map(|p|format!("{{\"SourceFile\":\"{}\",\"EXIF:SerialNumber\":\"cam\",\"MakerNotes:ShutterCount\":7}}",p.replace('\\',"\\\\").replace('"',"\\\""))).collect::<Vec<_>>();
    println!("[{}]",values.join(","));
}
