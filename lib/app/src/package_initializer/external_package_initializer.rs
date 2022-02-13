use crate::{
    common::file_path_resolver,
    infra::{FilePath, Infrastructure},
    package_build_script_compiler,
};
use std::error::Error;

pub fn initialize(
    infrastructure: &Infrastructure,
    package_directory: &FilePath,
    output_directory: &FilePath,
) -> Result<(), Box<dyn Error>> {
    for url in infrastructure
        .package_configuration_reader
        .read(package_directory)?
        .dependencies()
        .values()
    {
        initialize_recursively(infrastructure, url, output_directory)?;
    }

    Ok(())
}

fn initialize_recursively(
    infrastructure: &Infrastructure,
    package_url: &url::Url,
    output_directory: &FilePath,
) -> Result<(), Box<dyn Error>> {
    let package_directory =
        file_path_resolver::resolve_package_directory(output_directory, package_url);

    infrastructure
        .external_package_initializer
        .initialize(package_url, &package_directory)?;

    package_build_script_compiler::compile_external(
        infrastructure,
        package_url,
        output_directory,
        &file_path_resolver::resolve_external_package_build_script_file(
            output_directory,
            package_url,
            &infrastructure.file_path_configuration,
        ),
    )?;

    initialize(infrastructure, &package_directory, output_directory)?;

    Ok(())
}
