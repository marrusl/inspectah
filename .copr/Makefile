.PHONY: srpm

NAME     := inspectah
SPECFILE := $(NAME).spec
VERSION  := $(shell sed -n 's/^Version:[[:space:]]*//p' $(SPECFILE))
PYPROJECT_VERSION := $(shell sed -n 's/^version = "\(.*\)"/\1/p' pyproject.toml)
TARBALL  := $(NAME)-$(VERSION).tar.gz
RPMBUILD := $(HOME)/rpmbuild

srpm:
	test "$(VERSION)" = "$(PYPROJECT_VERSION)" || \
	    (echo "Version mismatch: $(SPECFILE)=$(VERSION) pyproject.toml=$(PYPROJECT_VERSION)" >&2; exit 1)
	dnf install -y rpm-build python3-devel python3-setuptools python3-wheel pyproject-rpm-macros
	mkdir -p $(RPMBUILD)/SOURCES $(RPMBUILD)/SPECS $(RPMBUILD)/SRPMS
	git archive --format=tar.gz --prefix=$(NAME)-$(VERSION)/ HEAD \
	    > $(RPMBUILD)/SOURCES/$(TARBALL)
	cp $(SPECFILE) $(RPMBUILD)/SPECS/
	rpmbuild -bs --define "_topdir $(RPMBUILD)" $(RPMBUILD)/SPECS/$(SPECFILE)
	cp $(RPMBUILD)/SRPMS/*.src.rpm $(outdir)/
