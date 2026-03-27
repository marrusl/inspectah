Name:           yoinkc
Version:        0.5.0
Release:        1%{?dist}
Summary:        Inspect RHEL/CentOS hosts and produce bootc image artifacts

License:        MIT
URL:            https://github.com/marrusl/yoinkc
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
yoinkc inspects package-based RHEL, CentOS, and Fedora hosts and produces
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
mkdir -p %{buildroot}%{_datadir}/yoinkc
touch %{buildroot}%{_datadir}/yoinkc/.packaged
%pyproject_save_files yoinkc

install -Dpm 0644 completions/yoinkc.bash \
    %{buildroot}%{_datadir}/bash-completion/completions/yoinkc
install -Dpm 0644 completions/yoinkc.zsh \
    %{buildroot}%{_datadir}/zsh/site-functions/_yoinkc
install -Dpm 0644 completions/yoinkc.fish \
    %{buildroot}%{_datadir}/fish/vendor_completions.d/yoinkc.fish

%check
%pytest

%files -f %{pyproject_files}
%license LICENSE
%doc README.md
%{_bindir}/yoinkc
%dir %{_datadir}/yoinkc
%{_datadir}/yoinkc/.packaged
%{_datadir}/bash-completion/completions/yoinkc
%{_datadir}/zsh/site-functions/_yoinkc
%{_datadir}/fish/vendor_completions.d/yoinkc.fish

%changelog
%autochangelog
