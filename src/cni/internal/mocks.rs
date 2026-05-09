use crate::cni::internal::{Link, LinkNotFoundError, NetlinkOps};

#[derive(Debug, Clone, Default)]
pub struct MockNetlinkOps {
    pub links: Vec<Link>,
    pub get_link_err: Option<LinkNotFoundError>,
}

impl NetlinkOps for MockNetlinkOps {
    fn get_link(&self, _namespace_path: &str, name: &str) -> Result<Link, LinkNotFoundError> {
        if let Some(error) = &self.get_link_err {
            return Err(error.clone());
        }

        self.links
            .iter()
            .find(|link| link.name == name)
            .cloned()
            .ok_or_else(|| LinkNotFoundError {
                device: name.to_string(),
            })
    }
}
