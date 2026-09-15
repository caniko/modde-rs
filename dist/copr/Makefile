.PHONY: srpm

VERSION = $(shell grep '^Version:' dist/rpm/modde.spec | awk '{print $$2}')

srpm:
	curl -Lo rs-modde-v$(VERSION).tar.gz \
		"https://github.com/caniko/rs-modde/archive/v$(VERSION).tar.gz"
	tar xf rs-modde-v$(VERSION).tar.gz
	cd rs-modde && cargo vendor vendor > ../cargo-vendor-config.toml
	tar czf vendor.tar.gz -C rs-modde vendor
	rpmbuild -bs dist/rpm/modde.spec \
		--define "_sourcedir $(CURDIR)" \
		--define "_srcrpmdir $(outdir)"
