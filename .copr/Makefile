.PHONY: srpm

NAME     := inspectah
SPECFILE := packaging/$(NAME).spec
VERSION  := $(shell sed -n 's/^Version:[[:space:]]*//p' $(SPECFILE))
TARBALL  := $(NAME)-$(VERSION).tar.gz
RPMBUILD := $(HOME)/rpmbuild

srpm:
	dnf install -y git rpm-build cargo rust nodejs npm
	mkdir -p $(RPMBUILD)/SOURCES $(RPMBUILD)/SPECS $(RPMBUILD)/SRPMS

	# Extract source tree into a staging directory
	$(eval STAGING := $(shell mktemp -d))
	git archive --format=tar.gz --prefix=$(NAME)-$(VERSION)/ HEAD \
	    | tar -xz -C $(STAGING)

	# Vendor Rust dependencies (COPR SRPM builder has network; Mock does not)
	cd $(STAGING)/$(NAME)-$(VERSION) && \
	    cargo vendor && \
	    mkdir -p .cargo && \
	    printf '[source.crates-io]\nreplace-with = "vendored-sources"\n\n[source.vendored-sources]\ndirectory = "vendor"\n' \
	        > .cargo/config.toml

	# Pre-build the web UI so Mock doesn't need nodejs
	cd $(STAGING)/$(NAME)-$(VERSION)/inspectah-web/ui && \
	    npm ci && \
	    npm run build && \
	    rm -rf node_modules

	# Create the tarball with vendored deps + pre-built UI
	tar -czf $(RPMBUILD)/SOURCES/$(TARBALL) \
	    -C $(STAGING) $(NAME)-$(VERSION)
	rm -rf $(STAGING)

	cp $(SPECFILE) $(RPMBUILD)/SPECS/$(NAME).spec
	rpmbuild -bs --define "_topdir $(RPMBUILD)" $(RPMBUILD)/SPECS/$(NAME).spec
	cp $(RPMBUILD)/SRPMS/*.src.rpm $(outdir)/
