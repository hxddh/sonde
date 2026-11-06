// gen-woodpecker writes Milvus Woodpecker log segments in the object-storage
// layout, encoded with Woodpecker's own codec (server/storage/codec): one
// object per block (<root>/<log>/<segment>/<block>.blk; block 0 starts with
// the header record), and for sealed segments footer.blk with the index
// records and the footer, as objectstorage.MinioFileWriter lays them out.
//
//	go run . <out-dir>
package main

import (
	"fmt"
	"hash/crc32"
	"os"
	"path/filepath"

	"github.com/zilliztech/woodpecker/server/storage/codec"
)

func write(path string, data []byte) {
	if err := os.MkdirAll(filepath.Dir(path), 0o755); err != nil {
		panic(err)
	}
	if err := os.WriteFile(path, data, 0o644); err != nil {
		panic(err)
	}
}

// segment writes `blocks` blocks of `perBlock` entries; sealed adds the footer.
func segment(root string, logID, segID int64, blocks, perBlock int, sealed bool) {
	dir := filepath.Join(root, fmt.Sprint(logID), fmt.Sprint(segID))
	var index []*codec.IndexRecord
	offset := int64(0)
	entry := int64(0)
	for b := 0; b < blocks; b++ {
		var obj []byte
		if b == 0 {
			obj = append(obj, codec.EncodeRecord(&codec.HeaderRecord{Version: codec.FormatVersion, FirstEntryID: 0})...)
		}
		var data []byte
		first := entry
		for i := 0; i < perBlock; i++ {
			data = append(data, codec.EncodeRecord(&codec.DataRecord{Payload: []byte(fmt.Sprintf("log %d segment %d entry %d", logID, segID, entry))})...)
			entry++
		}
		obj = append(obj, codec.EncodeRecord(&codec.BlockHeaderRecord{BlockNumber: int32(b), FirstEntryID: first, LastEntryID: entry - 1, BlockLength: uint32(len(data)), BlockCrc: crc32.ChecksumIEEE(data)})...)
		obj = append(obj, data...)
		write(filepath.Join(dir, fmt.Sprintf("%d.blk", b)), obj)
		index = append(index, &codec.IndexRecord{BlockNumber: int32(b), StartOffset: offset, BlockSize: uint32(len(obj)), FirstEntryID: first, LastEntryID: entry - 1})
		offset += int64(len(obj))
	}
	if !sealed {
		write(filepath.Join(dir, "write.lock"), []byte("gen-woodpecker"))
		return
	}
	var footer []byte
	for _, r := range index {
		footer = append(footer, codec.EncodeRecord(r)...)
	}
	idxLen := len(footer)
	footer = append(footer, codec.EncodeRecord(&codec.FooterRecord{TotalBlocks: int32(len(index)), TotalRecords: uint32(len(index)), TotalSize: uint64(offset), IndexLength: uint32(idxLen), Version: codec.FormatVersion, LAC: entry - 1})...)
	write(filepath.Join(dir, "footer.blk"), footer)
}

func main() {
	root := filepath.Join(os.Args[1], "woodpecker")
	segment(root, 1, 0, 3, 40, true)
	segment(root, 1, 1, 2, 25, true)
	segment(root, 1, 2, 2, 10, false)
	segment(root, 2, 0, 1, 5, true)
	fmt.Println("wrote", root)
}
