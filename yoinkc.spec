Name:           yoinkc
Version:        0.1.0
Release:        1%{?dist}
Summary:        Inspect RHEL/CentOS hosts and produce bootc image artifacts

License:        MIT
URL:            https://github.com/marrusl/yoinkc
Source0:        %{name}-%{version}.tar.gz
BuildArch:      noarch

BuildRequires:  pyproject-rpm-macros
BuildRequires:  python3-devel
Requires:       python3 >= 3.11
Requires:       python3-pydantic >= 2.0
Requires:       python3-jinja2 >= 3.1

%description
yoinkc inspects package-based RHEL/CentOS/Fedora hosts and produces bootc
image artifacts, including Containerfiles, config trees, and reports.

%prep
%autosetup -n %{name}-%{version}

%generate_buildrequires
%pyproject_buildrequires

%build
%pyproject_wheel

%install
%pyproject_install

# Mark packaged installs so runtime code can distinguish RPM/Homebrew builds
# from container and editable development installs.
install -d %{buildroot}%{_datadir}/yoinkc
touch %{buildroot}%{_datadir}/yoinkc/.packaged

%files -f %{pyproject_files}
%license LICENSE*
%doc README.md
%{_datadir}/yoinkc/.packaged

%changelog
