// flatten_microbench.cpp
//
// Mirrors the data-structure cost of pi-flash 0.2.0
// crates/app/src/services/file_tree.rs::flatten():
//   1) git_files.iter().map(|f| (f.path.clone(), f.status)).collect::<HashMap<_,_>>()
//   2) changed_ancestor_dirs(cwd, git_files)
//
// It intentionally keeps the original `cwd.join(porcelain_path)` behavior
// (including the repo-root-relative path bug) so the measured cost matches
// the current code, not a hypothetical fixed version.
//
// Input: a `git status --porcelain=v1 -z --untracked-files=all` dump.
// Usage: flatten_microbench <status-z.bin> <cwd>
// Build: g++ -O2 -std=c++17 flatten_microbench.cpp -o flatten_microbench
//
// This is a lower-bound proxy, not the app itself. C++ does not pay Rust's
// PathBuf hashing/allocation costs, so the real UI-thread cost is usually
// higher.

#include <chrono>
#include <cstdio>
#include <cstring>
#include <string>
#include <unordered_map>
#include <unordered_set>
#include <vector>

static long long elapsed_ms(std::chrono::steady_clock::time_point a,
                            std::chrono::steady_clock::time_point b) {
  return std::chrono::duration_cast<std::chrono::milliseconds>(b - a).count();
}

int main(int argc, char **argv) {
  if (argc != 3) {
    std::fprintf(stderr, "usage: %s <status-z.bin> <cwd>\n", argv[0]);
    return 2;
  }

  std::string cwd = argv[2];
  if (!cwd.empty() && cwd.back() != '/' && cwd.back() != '\\') {
    cwd += '/';
  }

  auto t0 = std::chrono::steady_clock::now();
  FILE *f = std::fopen(argv[1], "rb");
  if (!f) {
    std::perror("fopen");
    return 1;
  }
  std::fseek(f, 0, SEEK_END);
  const long size = std::ftell(f);
  std::fseek(f, 0, SEEK_SET);
  std::vector<char> buf(static_cast<size_t>(size));
  if (std::fread(buf.data(), 1, buf.size(), f) != buf.size()) {
    std::perror("fread");
    std::fclose(f);
    return 1;
  }
  std::fclose(f);
  auto t1 = std::chrono::steady_clock::now();

  // git_files -> HashMap<PathBuf, GitStatus> (current code: cwd.join(path))
  std::unordered_map<std::string, unsigned char> map;
  map.reserve(3000000);
  size_t records = 0;
  size_t start = 0;
  for (long i = 0; i < size; ++i) {
    if (buf[static_cast<size_t>(i)] != '\0') {
      continue;
    }
    const size_t len = static_cast<size_t>(i) - start;
    if (len >= 4) {
      // Skip the porcelain "XY " prefix; keep the path bytes.
      map.emplace(cwd + std::string(buf.data() + start + 3, len - 3), 0);
      ++records;
    }
    start = static_cast<size_t>(i) + 1;
  }
  auto t2 = std::chrono::steady_clock::now();

  // changed_ancestor_dirs(): walk every parent until cwd.
  std::unordered_set<std::string> ancestors;
  ancestors.reserve(400000);
  std::string cwd_noslash = cwd;
  while (!cwd_noslash.empty() &&
         (cwd_noslash.back() == '/' || cwd_noslash.back() == '\\')) {
    cwd_noslash.pop_back();
  }
  for (const auto &kv : map) {
    const std::string &p = kv.first;
    size_t pos = p.size();
    while (pos > 0) {
      pos = p.rfind('/', pos - 1);
      if (pos == std::string::npos) {
        break;
      }
      std::string dir = p.substr(0, pos);
      if (dir == cwd_noslash) {
        break;
      }
      ancestors.insert(std::move(dir));
    }
  }
  auto t3 = std::chrono::steady_clock::now();

  std::printf("records=%zu map=%zu fake_ancestor_dirs=%zu\n", records, map.size(),
              ancestors.size());
  std::printf(
      "read=%lldms parse+join+hashmap=%lldms ancestor_walk=%lldms total=%lldms\n",
      elapsed_ms(t0, t1), elapsed_ms(t1, t2), elapsed_ms(t2, t3),
      elapsed_ms(t0, t3));
  return 0;
}
