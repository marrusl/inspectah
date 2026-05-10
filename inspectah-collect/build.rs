fn main() {
    #[cfg(feature = "ffi-rpm")]
    {
        pkg_config::Config::new()
            .atleast_version("4.14")
            .probe("rpm")
            .expect(
                "librpm >= 4.14 not found. \
                 Install rpm-devel (RHEL/Fedora) or disable the ffi-rpm feature.",
            );
    }
}
