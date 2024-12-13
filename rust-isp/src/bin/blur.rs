fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = std::env::args().collect::<Vec<String>>();
    if args.len() != 3 {
        eprintln!("Usage: {} <kernel_size> <sigma>", args[0]);
        std::process::exit(1);
    }

    let kernel_size = args[1].parse::<usize>()?;
    let sigma = args[2].parse::<f32>()?;

    let kernel = generate_gaussian_kernel(kernel_size, sigma);
    
    // Print kernel as Rust const array declaration
    println!("const KERNEL: [[f32; {kernel_size}]; {kernel_size}] = [");
    for row in &kernel {
        print!("    [");
        for (i, val) in row.iter().enumerate() {
            print!("{:.10}", val);
            if i < row.len() - 1 {
                print!(", ");
            }
        }
        println!("],");
    }
    println!("];");
    
    Ok(())
}

fn generate_gaussian_kernel(size: usize, sigma: f32) -> Vec<Vec<f32>> {
    let mut kernel = vec![vec![0.0; size]; size];
    let mean = size as f32 / 2.0;
    let mut sum = 0.0;

    for x in 0..size {
        for y in 0..size {
            // 
            let exponent = -((x as f32 - mean).powi(2) + (y as f32 - mean).powi(2)) / (2.0 * sigma.powi(2));
            kernel[x][y] = (exponent.exp()) / (2.0 * std::f32::consts::PI * sigma.powi(2));
            sum += kernel[x][y];
        }
    }

    // Normalize the kernel
    for x in 0..size {
        for y in 0..size {
            kernel[x][y] /= sum;
        }
    }

    kernel
}
