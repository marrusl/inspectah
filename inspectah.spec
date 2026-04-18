Name:           inspectah
Version:        0.5.1
Release:        1%{?dist}
Summary:        Inspect RHEL/CentOS hosts and produce bootc image artifacts

License:        MIT
URL:            https://github.com/marrusl/inspectah
Source0:        %{url}/archive/v%{version}/%{name}-%{version}.tar.gz

BuildArch:      noarch

BuildRequires:  python3-devel
BuildRequires:  python3-setuptools
BuildRequires:  python3-wheel
BuildRequires:  pyproject-rpm-macros
BuildRequires:  python3-pytest

Requires:       python3 >= 3.11
Requires:       python3-pydantic >= 2.0
Requires:       python3-jinja2 >= 3.1
Requires:       podman

%description
inspectah inspects package-based RHEL, CentOS, and Fedora hosts and produces
bootc-compatible image artifacts including Containerfiles, configuration trees,
and migration reports.

%prep
%autosetup -n %{name}-%{version}

%generate_buildrequires
%pyproject_buildrequires

%build
%pyproject_wheel

%install
%pyproject_install
mkdir -p %{buildroot}%{_datadir}/inspectah
touch %{buildroot}%{_datadir}/inspectah/.packaged
%pyproject_save_files inspectah

install -Dpm 0644 completions/inspectah.bash \
    %{buildroot}%{_datadir}/bash-completion/completions/inspectah
install -Dpm 0644 completions/inspectah.zsh \
    %{buildroot}%{_datadir}/zsh/site-functions/_inspectah
install -Dpm 0644 completions/inspectah.fish \
    %{buildroot}%{_datadir}/fish/vendor_completions.d/inspectah.fish

%check
%pytest

%files -f %{pyproject_files}
%license LICENSE
%doc README.md
%{_bindir}/inspectah
%dir %{_datadir}/inspectah
%{_datadir}/inspectah/.packaged
%{_datadir}/bash-completion/completions/inspectah
%{_datadir}/zsh/site-functions/_inspectah
%{_datadir}/fish/vendor_completions.d/inspectah.fish

%changelog
%autochangelog
