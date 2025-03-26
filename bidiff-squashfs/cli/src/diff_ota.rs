use std::{
    collections::HashMap,
    path::{Path, PathBuf},
};

use color_eyre::{eyre::WrapErr as _, Result};
use tokio::{
    fs,
    io::{self, BufReader, BufWriter},
};
use tokio_util::sync::CancellationToken;
use tracing::info;

use crate::diff_plan::{ComponentId, DiffPlan, Operation};

const CLAIM_FILENAME: &str = "claim.json";

/// Preconditions:
/// - `out_dir` should be an empty directory.
pub async fn diff_ota(
    base_dir: &Path,
    top_dir: &Path,
    out_dir: &Path,
    cancel: CancellationToken,
) -> Result<()> {
    let _cancel_guard = cancel.drop_guard();
    for d in [base_dir, top_dir, out_dir] {
        assert!(
            fs::try_exists(d).await.unwrap_or(false),
            "{d:?} does not exist"
        );
        assert!(fs::metadata(d).await?.is_dir(), "{d:?} was not a directory");
    }

    let plan = make_plan(base_dir, top_dir)
        .await
        .wrap_err("failed to create diffing plan")?;
    info!("created diffing plan: {plan:#?}");

    execute_plan(&plan)
        .await
        .wrap_err("failed to execute diffing plan")
}

async fn make_plan(base_dir: &Path, top_dir: &Path) -> Result<DiffPlan> {
    let old_claim_path = base_dir.join(CLAIM_FILENAME);
    let new_claim_path = top_dir.join(CLAIM_FILENAME);

    let plan = crate::diff_plan::DiffPlan::new(&old_claim_path, &new_claim_path)
        .await
        .wrap_err("failed to create diffing plan from claim paths")?;

    Ok(plan)
}

async fn execute_plan(plan: &DiffPlan) -> Result<()> {
    for op in plan.0 {
        match op {
            Operation::Bidiff(id) => todo!(),
            Operation::Copy(id) => copy_component(),
            Operation::Delete(id) => todo!(),
            Operation::Create(id) => todo!(),
        }
    }

    todo!("plan execution")
}

#[derive(Debug, Eq, PartialEq, Clone)]
struct OtaFiles {
    ota_dir: PathBuf,
    claim: PathBuf,
    components: HashMap<ComponentId, PathBuf>,
}

#[derive(Debug, Eq, PartialEq, Clone, Copy, Hash)]
enum Location {
    Old,
    New,
    Patch,
}

/// FileContext is everything that describes the filesystem and other context
/// within which an operation will be performed. We keep it in one struct to make
/// it a little easier to pass to functions and work with.
#[derive(Debug)]
struct FileContext {
    old: OtaFiles,
    new: OtaFiles,
    patch: OtaFiles,
}

impl FileContext {
    fn new(old: OtaFiles, new: OtaFiles) -> Self {
        todo!()
    }

    fn files(&self, location: Location) -> &OtaFiles {
        match location {
            Location::Old => &self.old,
            Location::New => &self.new,
            Location::Patch => &self.patch,
        }
    }

    fn component_path(
        &self,
        location: Location,
        component: &ComponentId,
    ) -> Option<&Path> {
        self.files(location)
            .components
            .get(component)
            .map(|p| p.as_path())
    }
}

async fn copy_component(ctx: &FileContext, component: &ComponentId) -> Result<()> {
    let old_path = ctx
        .component_path(Location::Old, component)
        .expect("component was missing, but this should be impossible");
    let patch_path = ctx
        .component_path(Location::Out, component)
        .expect("component was missing, but this should be impossible");

    let mut old_file =
        BufReader::new(fs::File::open(old_path).await.wrap_err_with(|| {
            format!("failed to open old component at {}", old_path.display())
        })?);
    let mut new_file = BufWriter::new(
        fs::File::options()
            .create_new(true)
            .open(new_path)
            .await
            .wrap_err_with(|| {
                format!("failed to create new file at {}", new_path.display())
            })?,
    );

    io::copy_buf(&mut old_file, &mut new_file)
        .await
        .wrap_err("failed to copy over file")?;

    old_file
        .into_inner()
        .sync_all()
        .await
        .wrap_err_with(|| format!("failed to flush {old_path:?}"));
    new_file
        .into_inner()
        .sync_all()
        .await
        .wrap_err_with(|| format!("failed to flush {new_path:?}"));

    Ok(())
}
