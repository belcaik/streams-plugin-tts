use std::io;

fn main() -> io::Result<()> {
    io::copy(&mut io::stdin(), &mut io::sink())?;
    Ok(())
}
