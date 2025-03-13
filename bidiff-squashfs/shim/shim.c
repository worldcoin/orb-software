#include <fcntl.h>
#include <glib.h>
#include <glib/gchecksum.h>
#include <inttypes.h>
#include <sqfs/block.h>
#include <sqfs/compressor.h>
#include <sqfs/data_reader.h>
#include <sqfs/dir.h>
#include <sqfs/dir_reader.h>
#include <sqfs/error.h>
#include <sqfs/frag_table.h>
#include <sqfs/id_table.h>
#include <sqfs/inode.h>
#include <sqfs/io.h>
#include <sqfs/predef.h>
#include <sqfs/super.h>
#include <stdio.h>
#include <sys/mman.h>
#include <sys/stat.h>

// TODO: copied from data_reader.c
struct sqfs_data_reader_t {
  sqfs_object_t obj;

  sqfs_frag_table_t *frag_tbl;
  sqfs_compressor_t *cmp;
  sqfs_file_t *file;

  sqfs_u8 *data_block;
  size_t data_blk_size;
  sqfs_u64 current_block;

  sqfs_u8 *frag_block;
  size_t frag_blk_size;
  sqfs_u32 current_frag_index;
  sqfs_u32 block_size;

  sqfs_u8 scratch[];
};

typedef struct {
  sqfs_compressor_config_t cfg;
  sqfs_compressor_t *cmp;
  sqfs_super_t super;
  sqfs_file_t *file;
  sqfs_id_table_t *idtbl;
  sqfs_dir_reader_t *dr;
  sqfs_tree_node_t *root;
  sqfs_data_reader_t *data;

  sqfs_compressor_config_t options;
  bool have_options;
} sqfs_state_t;

void sqfs_perror(const char *file, const char *action, int error_code) {
  const char *errstr;

  switch (error_code) {
  case SQFS_ERROR_ALLOC:
    errstr = "out of memory";
    break;
  case SQFS_ERROR_IO:
    errstr = "I/O error";
    break;
  case SQFS_ERROR_COMPRESSOR:
    errstr = "internal compressor error";
    break;
  case SQFS_ERROR_INTERNAL:
    errstr = "internal error";
    break;
  case SQFS_ERROR_CORRUPTED:
    errstr = "data corrupted";
    break;
  case SQFS_ERROR_UNSUPPORTED:
    errstr = "unknown or not supported";
    break;
  case SQFS_ERROR_OVERFLOW:
    errstr = "numeric overflow";
    break;
  case SQFS_ERROR_OUT_OF_BOUNDS:
    errstr = "location out of bounds";
    break;
  case SFQS_ERROR_SUPER_MAGIC:
    errstr = "wrong magic value in super block";
    break;
  case SFQS_ERROR_SUPER_VERSION:
    errstr = "wrong squashfs version in super block";
    break;
  case SQFS_ERROR_SUPER_BLOCK_SIZE:
    errstr = "invalid block size specified in super block";
    break;
  case SQFS_ERROR_NOT_DIR:
    errstr = "target is not a directory";
    break;
  case SQFS_ERROR_NO_ENTRY:
    errstr = "no such file or directory";
    break;
  case SQFS_ERROR_LINK_LOOP:
    errstr = "hard link loop detected";
    break;
  case SQFS_ERROR_NOT_FILE:
    errstr = "target is not a file";
    break;
  case SQFS_ERROR_ARG_INVALID:
    errstr = "invalid argument";
    break;
  case SQFS_ERROR_SEQUENCE:
    errstr = "illegal oder of operations";
    break;
  default:
    errstr = "libsquashfs returned an unknown error code";
    break;
  }

  if (file != NULL)
    fprintf(stderr, "%s: ", file);

  if (action != NULL)
    fprintf(stderr, "%s: ", action);

  fprintf(stderr, "%s.\n", errstr);
}

static int open_sfqs(sqfs_state_t *state, const char *path) {
  int ret;

  state->file = sqfs_open_file(path, SQFS_FILE_OPEN_READ_ONLY);
  if (state->file == NULL) {
    perror(path);
    return -1;
  }

  ret = sqfs_super_read(&state->super, state->file);
  if (ret) {
    sqfs_perror(path, "reading super block", ret);
    goto fail_file;
  }

  sqfs_compressor_config_init(&state->cfg, state->super.compression_id,
                              state->super.block_size,
                              SQFS_COMP_FLAG_UNCOMPRESS);

  ret = sqfs_compressor_create(&state->cfg, &state->cmp);

#ifdef WITH_LZO
  if (state->super.compression_id == SQFS_COMP_LZO && ret != 0)
    ret = lzo_compressor_create(&state->cfg, &state->cmp);
#endif

  if (ret != 0) {
    sqfs_perror(path, "creating compressor", ret);
    goto fail_file;
  }

  if (state->super.flags & SQFS_FLAG_COMPRESSOR_OPTIONS) {
    ret = state->cmp->read_options(state->cmp, state->file);

    if (ret == 0) {
      state->cmp->get_configuration(state->cmp, &state->options);
      state->have_options = true;
    } else {
      sqfs_perror(path, "reading compressor options", ret);
      state->have_options = false;
    }
  } else {
    state->have_options = false;
  }

  state->idtbl = sqfs_id_table_create(0);
  if (state->idtbl == NULL) {
    sqfs_perror(path, "creating ID table", SQFS_ERROR_ALLOC);
    goto fail_cmp;
  }

  ret =
      sqfs_id_table_read(state->idtbl, state->file, &state->super, state->cmp);
  if (ret) {
    sqfs_perror(path, "loading ID table", ret);
    goto fail_id;
  }

  state->dr = sqfs_dir_reader_create(&state->super, state->cmp, state->file, 0);
  if (state->dr == NULL) {
    sqfs_perror(path, "creating directory reader", SQFS_ERROR_ALLOC);
    goto fail_id;
  }

  ret = sqfs_dir_reader_get_full_hierarchy(state->dr, state->idtbl, NULL, 0,
                                           &state->root);
  if (ret) {
    sqfs_perror(path, "loading filesystem tree", ret);
    goto fail_dr;
  }

  state->data = sqfs_data_reader_create(state->file, state->super.block_size,
                                        state->cmp, 0);
  if (state->data == NULL) {
    sqfs_perror(path, "creating data reader", SQFS_ERROR_ALLOC);
    goto fail_tree;
  }

  ret = sqfs_data_reader_load_fragment_table(state->data, &state->super);
  if (ret) {
    sqfs_perror(path, "loading fragment table", ret);
    goto fail_data;
  }

  return 0;
fail_data:
  sqfs_destroy(state->data);
fail_tree:
  sqfs_dir_tree_destroy(state->root);
fail_dr:
  sqfs_destroy(state->dr);
fail_id:
  sqfs_destroy(state->idtbl);
fail_cmp:
  sqfs_destroy(state->cmp);
fail_file:
  sqfs_destroy(state->file);
  return -1;
}

struct block {
  sqfs_u64 offset;
  sqfs_u32 size;
  sqfs_u32 pad;
};

struct block_with_hash {
  sqfs_u64 offset;
  sqfs_u32 size;
  char hash[32];
};

void get_all_inodes(sqfs_state_t *state, sqfs_tree_node_t *it, GPtrArray *ret) {
  sqfs_tree_node_t *child = it->children;
  while (child) {
    if (child->inode->base.type == SQFS_INODE_FILE ||
        child->inode->base.type == SQFS_INODE_EXT_FILE)
      g_ptr_array_add(ret, child->inode);
    get_all_inodes(state, child, ret);
    child = child->next;
  }
}

gint compare_inode_id(gconstpointer _a, gconstpointer _b) {
  sqfs_inode_generic_t const *a = _a;
  sqfs_inode_generic_t const *b = _b;

  return a->base.inode_number - b->base.inode_number;
}

/// remove duplicating elements from the pointer array
/// The array is assumed to be sorted
/// Takes ownership of the array and returns a new one without duplicates
GPtrArray *remove_duplicates_ptr_array(GPtrArray *const data,
                                       GCompareFunc cmp) {
  if (data->len <= 1) {
    return data;
  }
  GPtrArray *ret = g_ptr_array_new();
  for (unsigned i = 0; i < data->len - 1; i++) {
    gint tmp = cmp(g_ptr_array_index(data, i), g_ptr_array_index(data, i + 1));
    if (tmp > 0) {
      fprintf(stderr, "ptr array is not sorted at pos 0x%x\n", i);
      abort();
    }
    if (tmp < 0) {
      g_ptr_array_add(ret, g_ptr_array_index(data, i));
    }
  }
  g_ptr_array_add(ret, g_ptr_array_index(data, data->len - 1));
  g_ptr_array_free(data, TRUE);
  return ret;
}

/// remove duplicating elements from the pointer array
/// The array is assumed to be sorted
/// Takes ownership of the array and returns a new one without duplicates
GArray *remove_duplicates_blocks(GArray *const data) {
  if (data->len <= 1) {
    return data;
  }
  GArray *ret = g_array_new(TRUE, TRUE, sizeof(struct block));
  for (unsigned i = 0; i < data->len - 1; i++) {
    struct block const *a = &g_array_index(data, struct block, i);
    struct block const *b = &g_array_index(data, struct block, i + 1);
    if (a->offset > b->offset) {
      fprintf(stderr, "array is not sorted at pos 0x%x\n", i);
      abort();
    }
    // skip identical blocks
    if (a->offset == b->offset && a->size == b->size) {
      continue;
    }
    g_array_append_vals(ret, &g_array_index(data, struct block, i), 1);
  }
  g_array_append_vals(ret, &g_array_index(data, struct block, data->len - 1),
                      1);
  g_array_free(data, TRUE);
  return ret;
}

void get_file_inode_blocks(gpointer data, gpointer user_data) {
  sqfs_inode_generic_t *inode = data;
  GArray *blocks = user_data;
  sqfs_u64 location;
  sqfs_u32 frag_idx, frag_offset;
  sqfs_u64 size;
  switch (inode->base.type) {
  case SQFS_INODE_FILE:
  case SQFS_INODE_EXT_FILE:
    sqfs_inode_get_file_block_start(inode, &location);
    sqfs_inode_get_file_size(inode, &size);
    sqfs_inode_get_frag_location(inode, &frag_idx, &frag_offset);

    /* printf("Fragment index: 0x%X\n", frag_idx); */
    /* printf("Fragment offset: 0x%X\n", frag_offset); */
    /* printf("File size: %lu\n", (unsigned long)size); */

    /* if (inode->base.type == SQFS_INODE_EXT_FILE) { */
    /*     printf("Sparse: %" PRIu64 "\n", */
    /*            inode->data.file_ext.sparse); */
    /* } */

    /* printf("Blocks start: %lu\n", (unsigned long)location); */
    /* printf("Block count: %lu\n", */
    /*        (unsigned long)sqfs_inode_get_file_block_count(inode)); */

    for (unsigned long i = 0; i < sqfs_inode_get_file_block_count(inode); ++i) {
      /* printf("\tInode %lu Block #%lx start %lx size: %x (%s)\n",
       * inode->base.inode_number, (unsigned long)i, (unsigned long)location, */
      /*        SQFS_ON_DISK_BLOCK_SIZE(inode->extra[i]), */
      /*        SQFS_IS_BLOCK_COMPRESSED(inode->extra[i]) ? */
      /*        "compressed" : "uncompressed"); */
      if SQFS_IS_SPARSE_BLOCK (inode->extra[i]) {
        continue;
      }
      struct block b = {
          .size = SQFS_ON_DISK_BLOCK_SIZE(inode->extra[i]),
          .offset = location,
      };
      g_array_append_vals(blocks, &b, 1);
      location += SQFS_ON_DISK_BLOCK_SIZE(inode->extra[i]);
    }
    break;
  default:
    fprintf(stderr, "inode %u is not a file %u\n", inode->base.inode_number,
            inode->base.type);
    abort();
  }
}

static gint cmp_blocks(gconstpointer a, gconstpointer b) {
  struct block *block_a = (struct block *)a;
  struct block *block_b = (struct block *)b;

  if (block_a->offset < block_b->offset)
    return -1;
  else if (block_a->offset > block_b->offset)
    return 1;
  else
    return 0;
}

int shim_get_blocks(const char *path, struct block_with_hash **blocks,
                    size_t *blocks_len) {
  sqfs_state_t state = {0};

  if (open_sfqs(&state, path)) {
    fprintf(stderr, "open_sfqs");
    return 2;
  };

  GArray *blocks_and_fragments = g_array_new(TRUE, TRUE, sizeof(struct block));
  GPtrArray *inodes = g_ptr_array_new();
  get_all_inodes(&state, state.root, inodes);
  g_ptr_array_sort_values(inodes, compare_inode_id);
  inodes = remove_duplicates_ptr_array(inodes, compare_inode_id);

  // get all blocks
  g_ptr_array_foreach(inodes, get_file_inode_blocks, blocks_and_fragments);

  // get all fragments
  for (unsigned i = 0; i < sqfs_frag_table_get_size(state.data->frag_tbl);
       i++) {
    sqfs_fragment_t frag;
    sqfs_frag_table_lookup(state.data->frag_tbl, i, &frag);
    sqfs_u32 size = frag.size;
    if (SQFS_IS_SPARSE_BLOCK(size)) {
      fprintf(stderr, "fragment %u: sparse\n", i);
      abort();
    }

    sqfs_u32 on_disk_size = SQFS_ON_DISK_BLOCK_SIZE(size);

    struct block b = {
        .size = on_disk_size,
        .offset = frag.start_offset,
    };
    g_array_append_vals(blocks_and_fragments, &b, 1);
  }

  // make sure there is no gaps or overlaps
  g_array_sort(blocks_and_fragments, cmp_blocks);
  blocks_and_fragments = remove_duplicates_blocks(blocks_and_fragments);

  // TODO I hate opening the same file twice, but I don'd want to import the
  // sqfs_file_stdio_t
  int fd = open(path, O_RDONLY);
  struct stat sb;
  fstat(fd, &sb);
  const char *file_map = mmap(NULL, sb.st_size, PROT_READ, MAP_PRIVATE, fd, 0);

  for (unsigned long i = 0; i + 1 < blocks_and_fragments->len; i++) {
    struct block *c = &g_array_index(blocks_and_fragments, struct block, i);
    struct block *n = &g_array_index(blocks_and_fragments, struct block, i + 1);
    uint64_t end = c->offset + c->size;
    if (end < n->offset) {
      printf("gap between blocks %lx %x and %lx %x\n", c->offset, c->size,
             n->offset, n->size);
    } else if (end > n->offset) {
      printf("overlap between blocks %lx %x and %lx %x\n", c->offset, c->size,
             n->offset, n->size);
    }
  }

  // calculate hash of each chunk
  GChecksum *checksum = g_checksum_new(G_CHECKSUM_SHA256);
  GArray *blocks_with_hash =
      g_array_new(TRUE, TRUE, sizeof(struct block_with_hash));

  for (unsigned long i = 0; i < blocks_and_fragments->len; i++) {
    struct block *b = &g_array_index(blocks_and_fragments, struct block, i);
    g_checksum_update(checksum, (guchar *)file_map + b->offset, b->size);
    // printf("block %u: start %u size %u sha256: %s\n", i, b->offset, b->size,
    // g_checksum_get_string(checksum));
    struct block_with_hash bh = {
        .offset = b->offset,
        .size = b->size,
    };
    gsize sz = sizeof(bh.hash);
    g_checksum_get_digest(checksum, (unsigned char *)bh.hash, &sz);
    if (sz != sizeof(bh.hash)) {
      abort();
    }
    g_array_append_vals(blocks_with_hash, &bh, 1);
    g_checksum_reset(checksum);
  }
  *blocks = g_array_steal(blocks_with_hash, blocks_len);
  return 0;
}

uint64_t shim_get_inode_table_idx(const char *path) {
  sqfs_state_t state = {0};

  if (open_sfqs(&state, path)) {
    fprintf(stderr, "open_sfqs");
    return 0;
  };

  return state.super.inode_table_start;
}
