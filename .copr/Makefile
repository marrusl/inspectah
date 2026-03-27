.PHONY: srpm

NAME     := yoinkc
VERSION  := $(shell grep '^version' pyproject.toml | sed 's/version = "\(.*\)"/\1/')
TARBALL  := $(NAME)-$(VERSION).tar.gz
SPECFILE := $(NAME).spec
RPMBUILD := $(HOME)/rpmbuild

srpm:
	dnf install -y rpm-build python3-devel python3-setuptools python3-wheel pyproject-rpm-macros
	mkdir -p $(RPMBUILD)/SOURCES $(RPMBUILD)/SPECS $(RPMBUILD)/SRPMS
	git archive --format=tar.gz --prefix=$(NAME)-$(VERSION)/ HEAD \
	    > $(RPMBUILD)/SOURCES/$(TARBALL)
	cp $(SPECFILE) $(RPMBUILD)/SPECS/
	rpmbuild -bs --define "_topdir $(RPMBUILD)" $(RPMBUILD)/SPECS/$(SPECFILE)
	cp $(RPMBUILD)/SRPMS/*.src.rpm $(outdir)/
