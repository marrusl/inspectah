.PHONY: srpm

NAME     := inspectah
SPECFILE := packaging/$(NAME).spec
VERSION  := $(shell sed -n 's/^Version:[[:space:]]*//p' $(SPECFILE))
TARBALL  := $(NAME)-$(VERSION).tar.gz
RPMBUILD := $(HOME)/rpmbuild

srpm:
	dnf install -y git rpm-build golang
	mkdir -p $(RPMBUILD)/SOURCES $(RPMBUILD)/SPECS $(RPMBUILD)/SRPMS
	git archive --format=tar.gz --prefix=$(NAME)-$(VERSION)/ HEAD \
	    > $(RPMBUILD)/SOURCES/$(TARBALL)
	cp $(SPECFILE) $(RPMBUILD)/SPECS/$(NAME).spec
	rpmbuild -bs --define "_topdir $(RPMBUILD)" $(RPMBUILD)/SPECS/$(NAME).spec
	cp $(RPMBUILD)/SRPMS/*.src.rpm $(outdir)/
