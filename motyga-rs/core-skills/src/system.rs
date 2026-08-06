pub(crate) use motyga_skills::install_system_skills;
pub(crate) use motyga_skills::system_cache_root_dir;

use motyga_utils_absolute_path::AbsolutePathBuf;

pub(crate) fn uninstall_system_skills(motyga_home: &AbsolutePathBuf) {
    let _ = std::fs::remove_dir_all(system_cache_root_dir(motyga_home));
}
