Name:           inspectah
Version:        0.1.0
Release:        1%{?dist}
Summary:        Inspect package-mode hosts and produce bootc image artifacts

License:        MIT
URL:            https://github.com/marrusl/inspectah
Source0:        %{name}-%{version}.tar.gz

BuildRequires:  golang >= 1.21

Requires:       podman >= 4.4

Conflicts:      python3-inspectah

%description
inspectah inspects package-based RHEL, CentOS, and Fedora hosts and
produces bootc-compatible image artifacts including Containerfiles,
configuration trees, and migration reports.

The inspectah binary manages the container lifecycle transparently.
Install via dnf, run inspectah scan, and the tool handles image
pulling, host inspection, and artifact generation.

%prep
%autosetup -n %{name}-%{version}

%build
cd cmd/inspectah
export GOFLAGS=-mod=vendor
export GONOSUMCHECK=1
export GONOSUMDB=*
go build -trimpath -ldflags "-s -w \
    -X main.version=%{version} \
    -X main.commit=%{?_commit}%{!?_commit:unknown} \
    -X main.date=$(date -u +%%Y-%%m-%%dT%%H:%%M:%%SZ)" \
    -o inspectah .

%install
install -Dpm 0755 cmd/inspectah/inspectah %{buildroot}%{_bindir}/inspectah

# Generate and install shell completions
%{buildroot}%{_bindir}/inspectah completion bash > inspectah.bash
%{buildroot}%{_bindir}/inspectah completion zsh > _inspectah
%{buildroot}%{_bindir}/inspectah completion fish > inspectah.fish

install -Dpm 0644 inspectah.bash \
    %{buildroot}%{_datadir}/bash-completion/completions/inspectah
install -Dpm 0644 _inspectah \
    %{buildroot}%{_datadir}/zsh/site-functions/_inspectah
install -Dpm 0644 inspectah.fish \
    %{buildroot}%{_datadir}/fish/vendor_completions.d/inspectah.fish

%files
%license LICENSE
%doc README.md
%{_bindir}/inspectah
%{_datadir}/bash-completion/completions/inspectah
%{_datadir}/zsh/site-functions/_inspectah
%{_datadir}/fish/vendor_completions.d/inspectah.fish

%changelog
%autochangelog
