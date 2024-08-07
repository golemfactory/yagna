use ya_service_bus::typed as bus;

pub const BUS_LOCAL: &str = "/local";
pub const BUS_PUBLIC: &str = "/public";

#[derive(Clone)]
pub struct GsbBindPoints {
    public: bus::Endpoint,
    local: bus::Endpoint,
}

impl GsbBindPoints {
    pub fn new(public: &str, local: &str) -> Self {
        Self {
            public: bus::service(public),
            local: bus::service(local),
        }
    }

    pub fn service(&self, name: &str) -> GsbBindPoints {
        Self {
            public: bus::service(format!("{}/{name}", self.public.addr())),
            local: bus::service(format!("{}/{name}", self.local.addr())),
        }
    }

    // Returns local endpoint relative to give GSB binding point.
    pub fn endpoint(&self, name: &str) -> bus::Endpoint {
        bus::service(format!("{}/{name}", self.local.addr()))
    }

    pub fn prefix(&self, prefix: &str) -> GsbBindPoints {
        Self {
            public: bus::service(format!("{prefix}{}", self.public.addr())),
            local: bus::service(format!("{prefix}{}", self.local.addr())),
        }
    }

    pub fn public(&self) -> bus::Endpoint {
        self.public.clone()
    }

    pub fn local(&self) -> bus::Endpoint {
        self.local.clone()
    }

    pub fn local_addr(&self) -> &str {
        self.local.addr()
    }

    pub fn public_addr(&self) -> &str {
        self.public.addr()
    }
}

impl Default for GsbBindPoints {
    fn default() -> Self {
        Self::new(BUS_PUBLIC, BUS_LOCAL)
    }
}

impl std::fmt::Debug for GsbBindPoints {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("GsbBindPoints")
            .field("public", &self.public.addr())
            .field("local", &self.local.addr())
            .finish()
    }
}
