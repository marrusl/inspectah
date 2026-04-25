Name:           inspectah-cli
Version:        0.1.0
Release:        1%{?dist}
Summary:        Native CLI wrapper for inspectah container-based migration tool

License:        MIT
URL:            https://github.com/marrusl/inspectah
Source0:        %{name}-%{version}.tar.gz

BuildRequires:  golang >= 1.21

Requires:       podman >= 4.4

%description
inspectah-cli is a native Go wrapper around the inspectah container image.
It provides a polished CLI experience with tab completion, progress
indicators, and structured error messages for inspecting package-mode
RHEL/CentOS hosts and producing bootc image artifacts.

On systems where inspectah's Python dependencies are unavailable (RHEL 8,
RHEL 9), this wrapper runs inspectah inside its container image
transparently.

%prep
%autosetup -n %{name}-%{version}

%build
cd cmd/inspectah
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
