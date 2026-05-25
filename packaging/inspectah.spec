%global debug_package %{nil}

Name:           inspectah
Version:        0.8.1~alpha.3
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

%files
%license LICENSE
%doc README.md
%{_bindir}/inspectah

%changelog
%autochangelog
