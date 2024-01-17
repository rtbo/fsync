use fsync::path::PathBuf;
use fsyncd::storage::Storage;
use libtest_mimic::Failed;
use futures::FutureExt;

use crate::Harness;

macro_rules! test {
    (
        fn $name:ident ($harness: ident) { $( $stmts: stmt;)* }
    )=> {
        pub async fn $name<L, R>($harness: Harness<L, R>) -> Result<(), Failed>
        where
            L: Storage,
            R: Storage,
        {
            $(
                $stmts
            )+
            Ok(())
        }
    };
    (
        #[should_fail]
        fn $name:ident ($harness: ident) { $( $stmts: stmt;)* }
    )=> {
        pub async fn $name<L, R>($harness: Harness<L, R>) -> Result<(), Failed>
        where
            L: Storage,
            R: Storage,
        {
            let fut = std::panic::AssertUnwindSafe(async move {
                $(
                    $stmts
                )+
                Ok::<(), Failed>(())
            });
            let res = fut.catch_unwind().await;
            match res {
                Ok(Ok(())) => Err(Failed::from(concat!(stringify!($name), " expected to fail but succeeded"))),
                Ok(Err(_)) => Ok(()),
                Err(_) => Ok(()),
            }
        }
    }
}

test!(
    fn copy_remote_to_local(harness) {
        let path = PathBuf::from("/only-remote.txt");

        harness.service.copy_remote_to_local(&path).await?;

        let content = harness.local_file_content(&path).await?;
        assert_eq!(&content, path.as_str());
    }
);

test!(
    #[should_fail]
    fn copy_remote_to_local_fail_missing(harness) {
        let path = PathBuf::from("/not-a-file.txt");
        harness.service.copy_remote_to_local(&path).await?;
    }
);

test!(
    #[should_fail]
    fn copy_remote_to_local_fail_relative(harness) {
        let path = PathBuf::from("only-remote.txt");
        harness.service.copy_remote_to_local(&path).await?;
    }
);
