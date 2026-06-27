%global debug_package %{nil}

Name:           inspectah
Version:        0.8.6~beta.5
Release:        1%{?dist}
Summary:        Inspect package-mode hosts and produce bootc image artifacts

License:        MIT
URL:            https://github.com/marrusl/inspectah
Source0:        %{name}-%{version}.tar.gz

BuildRequires:  cargo
BuildRequires:  rust

Requires:       podman >= 4.4

%description
inspectah inspects package-based RHEL, CentOS, and Fedora hosts and
produces bootc-compatible image artifacts including Containerfiles,
configuration trees, and migration reports.

Install via dnf and run inspectah scan. The tool handles host
inspection and artifact generation directly.

%prep
%autosetup -n %{name}-%{version}

%build
cargo build --release -p inspectah-cli

%install
install -Dpm 0755 target/release/inspectah %{buildroot}%{_bindir}/inspectah
# Generate shell completions from the binary itself
mkdir -p %{buildroot}%{_datadir}/bash-completion/completions
mkdir -p %{buildroot}%{_datadir}/zsh/site-functions
mkdir -p %{buildroot}%{_datadir}/fish/vendor_completions.d
target/release/inspectah completions bash > %{buildroot}%{_datadir}/bash-completion/completions/inspectah
target/release/inspectah completions zsh > %{buildroot}%{_datadir}/zsh/site-functions/_inspectah
target/release/inspectah completions fish > %{buildroot}%{_datadir}/fish/vendor_completions.d/inspectah.fish

%files
%license LICENSE
%doc README.md
%{_bindir}/inspectah
%{_datadir}/bash-completion/completions/inspectah
%{_datadir}/zsh/site-functions/_inspectah
%{_datadir}/fish/vendor_completions.d/inspectah.fish

%changelog
%autochangelog
