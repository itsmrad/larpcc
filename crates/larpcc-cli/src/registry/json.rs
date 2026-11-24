use super::{Error, Profile, Registry, store};
use serde_json::{Value, json};

fn binding(path: Option<&std::path::Path>, label: Option<&str>, state: &str) -> Value {
    json!({"path":path.map(super::display::path),"recorded_version":label,"detected_version":null,"state":state,"qualified":false})
}
// Display is path presence/type only; full signing validation belongs to bind/open.
fn desktop_state(binding: Option<&super::model::DesktopBinding>) -> &'static str {
    binding
        .map(|b| {
            if crate::desktop::binding_path_present(b.provider, &b.path) {
                "ok"
            } else {
                "invalid"
            }
        })
        .unwrap_or("unbound")
}
impl Registry {
    fn profile_json(&self, p: &Profile, data: &store::Data) -> Result<Value, Error> {
        let container = self.root().join("profiles").join(p.id.to_string());
        let surface = |which: &str, enabled| -> Result<Value, Error> {
            let path = if data.version == 3 {
                container.join(which)
            } else {
                container.clone()
            };
            let container_status = crate::platform::inspect_home(&container)?;
            let state = match if container_status == super::DirStatus::Ok {
                crate::platform::inspect_home(&path)?
            } else {
                container_status
            } {
                super::DirStatus::Ok => "ok",
                super::DirStatus::Missing => "missing",
                super::DirStatus::Unsafe(_) => "unsafe",
            };
            Ok(json!({"enabled":enabled,"path":super::display::path(&path),"state":state}))
        };
        let mut aliases: Vec<_> = data
            .aliases
            .iter()
            .filter(|a| a.profile_id == p.id)
            .collect();
        aliases.sort_by(|a, b| a.name.cmp(&b.name));
        let aliases: Vec<_> = aliases
            .into_iter()
            .map(|a| super::alias::alias_json(self.root(), a))
            .collect();
        let desktop = data
            .desktop_bindings
            .iter()
            .find(|b| b.provider == p.provider);
        let enabled = p.surfaces.unwrap_or_default();
        Ok(
            json!({"id":p.id.to_string(),"name":p.name,"provider":p.provider,"state":p.state.unwrap_or(super::ProfileState::Active),
            "surfaces":{"cli":surface("cli",enabled.cli)?,"desktop":surface("desktop",enabled.desktop)?},"aliases":aliases,
            "desktop_binding":binding(desktop.map(|b| b.path.as_path()),desktop.and_then(|b| b.recorded_version.as_deref()),desktop_state(desktop))}),
        )
    }
    pub fn metadata_json(
        &self,
        command: &str,
        name: Option<&super::ProfileName>,
    ) -> Result<Value, Error> {
        let _ancestors = crate::platform::anchor(self.root())?.ancestors;
        let (data, _lock) = store::snapshot(self.root(), self.expected)?;
        self.observed.set(Some(data.revision));
        let mut profiles: Vec<_> = data.profiles.iter().collect();
        profiles.sort_by(|a, b| a.name.cmp(&b.name));
        if command == "profile.list" {
            return Ok(
                json!({"profiles":profiles.into_iter().map(|p| self.profile_json(p,&data)).collect::<Result<Vec<_>,_>>()?}),
            );
        }
        if command == "profile.show" {
            let name = name.ok_or(Error::RecoveryRequired)?;
            let p = profiles
                .into_iter()
                .find(|p| &p.name == name)
                .ok_or_else(|| Error::NotFound { name: name.clone() })?;
            return self.profile_json(p, &data);
        }
        let bindings: Vec<_> = super::Provider::ALL.into_iter().flat_map(|p| {
            let b = data.bindings.iter().find(|b| b.provider == p);
            let desktop = data.desktop_bindings.iter().find(|b| b.provider == p);
            let state = b.map(|b| if crate::provider::validate(p,&b.executable,self.root()).is_ok(){"ok"}else{"invalid"}).unwrap_or("unbound");
            [json!({"provider":p,"surface":"cli","binding":binding(b.map(|b| b.executable.as_path()),None,state)}),
             json!({"provider":p,"surface":"desktop","binding":binding(desktop.map(|b| b.path.as_path()),desktop.and_then(|b| b.recorded_version.as_deref()),desktop_state(desktop))})]
        }).collect();
        Ok(json!({"bindings":bindings}))
    }
    pub(crate) fn with_status_metadata<T>(
        &self,
        name: &super::ProfileName,
        expected_id: Option<u64>,
        prepare: impl FnOnce(&Value) -> Result<T, Error>,
    ) -> Result<(Value, T), Error> {
        let _ancestors = crate::platform::anchor(self.root())?.ancestors;
        let (data, _lock) = store::snapshot(self.root(), self.expected)?;
        self.observed.set(Some(data.revision));
        let p = data.profiles.iter().find(|p| &p.name == name);
        if expected_id.is_some_and(|id| p.is_none_or(|p| p.id.get() != id)) {
            return Err(Error::Conflict);
        }
        let p = p.ok_or_else(|| Error::NotFound { name: name.clone() })?;
        let profile = self.profile_json(p, &data)?;
        let cli = data.bindings.iter().find(|b| b.provider == p.provider);
        let state = match cli {
            Some(b)
                if crate::provider::validate(b.provider, &b.executable, self.root()).is_ok() =>
            {
                "ok"
            }
            Some(_) => "invalid",
            None => "unbound",
        };
        let metadata = json!({"profile":profile,"detected_versions":[{"provider":p.provider,"surface":"cli","binding":binding(cli.map(|b| b.executable.as_path()),None,state)}],"native_auth":"not_observed","native_rows":[],"diagnostics":[]});
        let prepared = prepare(&metadata)?;
        Ok((metadata, prepared))
    }
}
