// The modules the Go fixes sonde prints are compiled against
// (crates/analysis/tests/fix_code.rs): named here so `go mod tidy` keeps them.
package fixcheck

import (
	_ "github.com/aws/aws-sdk-go-v2/aws"
	_ "github.com/aws/aws-sdk-go-v2/config"
	_ "github.com/aws/aws-sdk-go-v2/feature/s3/manager"
	_ "github.com/aws/aws-sdk-go-v2/service/s3"
	_ "golang.org/x/sync/errgroup"
)
