.PHONY: srpm

VERSION = $(shell grep '^Version:' dist/rpm/modde.spec | awk '{print $$2}')

srpm:
	curl -Lo modde-rs-v$(VERSION).tar.gz \
		"https://github.com/caniko/modde-rs/archive/v$(VERSION).tar.gz"
	tar xf modde-rs-v$(VERSION).tar.gz
	cd modde-rs && cargo vendor vendor > ../cargo-vendor-config.toml
	tar czf vendor.tar.gz -C modde-rs vendor
	rpmbuild -bs dist/rpm/modde.spec \
		--define "_sourcedir $(CURDIR)" \
		--define "_srcrpmdir $(outdir)"
