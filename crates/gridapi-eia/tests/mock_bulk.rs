//! Offline tests of bulk-file downloads against a scripted local server.
#![cfg(feature = "bulk")]
#![allow(clippy::expect_used, clippy::panic)]

mod common;

use std::time::Duration;

use gridapi_eia::bulk::eia860m::Release;
use gridapi_eia::bulk::eia930::{Dataset, HalfYear, SixMonthFile};
use gridapi_eia::{Client, Error, RetryPolicy};

use common::{MockServer, Reply};

fn client(server: &MockServer) -> Client {
    Client::builder()
        .api_key("unused-for-bulk")
        .base_url(format!("{}/v2", server.base))
        .bulk_base_url(server.base.clone())
        .retry(RetryPolicy {
            max_retries: 2,
            base_delay: Duration::from_millis(1),
            max_delay: Duration::from_millis(3),
        })
        .timeout(Duration::from_secs(5))
        .build()
        .expect("client")
}

fn redirect() -> Reply {
    Reply::bytes(301, "").with_header("Location", "/electricity/data/eia860m/")
}

#[tokio::test]
async fn downloads_to_dest_and_leaves_no_part_file() {
    let server = MockServer::script(vec![Reply::bytes(
        200,
        b"PK\x03\x04fake-xlsx-body".to_vec(),
    )]);
    let dir = tempfile::tempdir().expect("tmp");
    let dest = dir.path().join("g.xlsx");
    let n = client(&server)
        .download_eia860m(Release::new(2026, 9).expect("release"), &dest)
        .await
        .expect("download");
    assert_eq!(n, 18);
    assert_eq!(
        std::fs::read(&dest).expect("read"),
        b"PK\x03\x04fake-xlsx-body"
    );
    assert!(!dir.path().join("g.xlsx.part").exists());
    assert_eq!(
        server.seen(),
        ["/electricity/data/eia860m/xls/september_generator2026.xlsx"]
    );
}

#[tokio::test]
async fn falls_back_to_archive_path_when_not_in_xls() {
    let server = MockServer::start(|_, url| {
        if url.contains("/archive/xls/") {
            Reply::bytes(200, b"PK\x03\x04ok".to_vec())
        } else {
            Reply::bytes(404, "")
        }
    });
    let dir = tempfile::tempdir().expect("tmp");
    let dest = dir.path().join("g.xlsx");
    client(&server)
        .download_eia860m(Release::new(2026, 8).expect("release"), &dest)
        .await
        .expect("download");
    assert_eq!(server.seen().len(), 2);
}

#[tokio::test]
async fn redirect_means_not_published_and_is_not_followed() {
    let server = MockServer::script(vec![redirect()]);
    let dir = tempfile::tempdir().expect("tmp");
    let dest = dir.path().join("g.xlsx");
    let err = client(&server)
        .download_eia860m(Release::new(2026, 12).expect("release"), &dest)
        .await
        .expect_err("not published");
    assert!(matches!(err, Error::NotPublished(_)), "{err:?}");
    assert!(!dest.exists());
    assert_eq!(
        server.seen().len(),
        2,
        "only the two candidate URLs, no redirect hop"
    );
}

#[tokio::test]
async fn html_served_with_200_is_rejected_and_cleaned_up() {
    let server = MockServer::script(vec![Reply::bytes(200, "<html>Not found</html>")]);
    let dir = tempfile::tempdir().expect("tmp");
    let dest = dir.path().join("g.xlsx");
    let err = client(&server)
        .download_eia860m(Release::new(2026, 12).expect("release"), &dest)
        .await
        .expect_err("not a workbook");
    assert!(matches!(err, Error::NotPublished(_)), "{err:?}");
    assert!(!dest.exists());
    assert!(!dir.path().join("g.xlsx.part").exists());
}

#[tokio::test]
async fn latest_walks_back_to_the_newest_published_month() {
    let server = MockServer::start(|_, url| {
        if url.contains("september_generator2026") && url.contains("/archive/") {
            Reply::bytes(200, b"PK\x03\x04sep".to_vec())
        } else {
            redirect()
        }
    });
    let dir = tempfile::tempdir().expect("tmp");
    let dest = dir.path().join("latest.xlsx");
    let got = client(&server)
        .download_latest_eia860m(Release::new(2026, 11).expect("release"), &dest)
        .await
        .expect("latest");
    assert_eq!(
        got,
        Release {
            year: 2026,
            month: 9
        }
    );
    assert_eq!(std::fs::read(&dest).expect("read"), b"PK\x03\x04sep");
}

#[tokio::test]
async fn latest_gives_up_after_six_months() {
    let server = MockServer::script(vec![redirect()]);
    let dir = tempfile::tempdir().expect("tmp");
    let err = client(&server)
        .download_latest_eia860m(
            Release::new(2026, 11).expect("release"),
            &dir.path().join("x.xlsx"),
        )
        .await
        .expect_err("none");
    assert!(matches!(err, Error::NotPublished(_)));
    assert_eq!(server.seen().len(), 12, "6 months x 2 candidate URLs");
}

#[tokio::test]
async fn eia930_download_streams_a_csv() {
    let csv = "Balancing Authority,Data Date\nAECI,07/01/2026\n";
    let server = MockServer::script(vec![Reply::bytes(200, csv)]);
    let dir = tempfile::tempdir().expect("tmp");
    let dest = dir.path().join("b.csv");
    let file = SixMonthFile {
        dataset: Dataset::Balance,
        year: 2026,
        half: HalfYear::JulDec,
    };
    let n = client(&server)
        .download_eia930(file, &dest)
        .await
        .expect("download");
    assert_eq!(n, u64::try_from(csv.len()).expect("len"));
    assert_eq!(
        server.seen(),
        ["/electricity/gridmonitor/sixMonthFiles/EIA930_BALANCE_2026_Jul_Dec.csv"]
    );
    assert_eq!(std::fs::read_to_string(&dest).expect("read"), csv);
}

#[tokio::test]
async fn eia930_missing_half_is_not_published() {
    let server = MockServer::script(vec![Reply::bytes(404, "")]);
    let dir = tempfile::tempdir().expect("tmp");
    let file = SixMonthFile {
        dataset: Dataset::Interchange,
        year: 2030,
        half: HalfYear::JanJun,
    };
    let err = client(&server)
        .download_eia930(file, &dir.path().join("i.csv"))
        .await
        .expect_err("404");
    assert!(matches!(err, Error::NotPublished(_)));
}
