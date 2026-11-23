# Linux-only directory inventory. Proxmox ships Perl with its native tools.
use strict;
use warnings;
use Fcntl qw(O_DIRECTORY O_NOFOLLOW);

# Linux x86-64 and arm64 use this O_PATH flag. Opening a terminal automount
# with it gets a descriptor without triggering the mount.
use constant O_PATH => 1 << 21;

my ($root, $directory, $mount, $backend, $offline) = @ARGV;
exit 2 unless defined($offline) && $root =~ m{\A/};
my @root = grep { length } split m{/}, $root;
my @parts = (@root, grep { length } split m{/}, $directory);
exit 2 if grep { $_ eq '.' || $_ eq '..' } @parts;

# Read the kernel mount table without resolving paths or waking automounts.
sub read_mounts {
    open(my $info, '<', '/proc/self/mountinfo') or exit 2;
    my (%mounts, %records);
    while (my $line = <$info>) {
        my ($location, $details) = split / - /, $line, 2;
        next unless defined $details;
        my ($id, undef, $device, $root, $target) = split / /, $location;
        my ($fs, $source) = split / /, $details;
        next unless defined($source) && defined($target);
        $target =~ s/\\([0-7]{3})/chr(oct($1))/ge;
        push @{$mounts{$target}}, $id;
        $records{$id} = [$fs, join(' ', $device, $root, $fs, $source)];
    }
    close($info);
    return (\%mounts, \%records);
}

my ($mounts, $records) = read_mounts();
exit 0 unless $mounts->{'/'};

if (length $mount) {
    $mount = '/' . join('/', grep { length } split m{/}, $mount);
    my $root_path = '/' . join('/', @root);
    exit 0 unless $mount eq '/' || $mount eq $root_path || index($root_path, "$mount/") == 0;
    my $ids = $mounts->{$mount} or exit 0;
    exit 0 unless allowed($ids);
    my %types = (
        nfs => qr/\Anfs4?\z/,
        cifs => qr/\A(?:cifs|smb3)\z/,
        cephfs => qr/\A(?:ceph|fuse\.ceph)\z/,
        glusterfs => qr/\Afuse\.glusterfs\z/,
    );
    my $expected = $types{$backend};
    exit 0 if $expected && $records->{$ids->[0]}->[0] !~ /$expected/;
}

sub allowed {
    my ($mount_ids) = @_;
    return 1 unless $mount_ids;
    return 0 unless @$mount_ids == 1;
    my $fs = $records->{$mount_ids->[0]}->[0];
    return 0 if $fs eq 'autofs';
    return 1 unless $offline;
    return $fs =~ /\A(?:ext[234]|xfs|zfs|btrfs|overlay|tmpfs|ramfs|bcachefs|f2fs)\z/;
}

sub mount_id {
    my ($handle) = @_;
    my $fd = fileno($handle);
    open(my $info, '<', "/proc/self/fdinfo/$fd") or return;
    while (my $line = <$info>) {
        return $1 if $line =~ /\Amnt_id:\s*(\d+)\s*\z/;
    }
    return;
}
sub verify_mount {
    my ($path, $id) = @_;
    my $expected = $mounts->{$path};
    return 0 unless defined($id) && $expected && @$expected == 1 && $expected->[0] eq $id;
    # The descriptor pins this mount while we check that its live record is
    # still the sole mount at this path, including its device and source.
    my ($live, $identities) = read_mounts();
    my $ids = $live->{$path};
    return $ids && @$ids == 1 && $ids->[0] eq $id
        && $identities->{$id}->[1] eq $records->{$id}->[1];
}
exit 0 unless allowed($mounts->{'/'});

# Keep each directory open while opening its child. Check the actual mount ID
# before traversing it so a change since mountinfo was read fails closed.
my $flags = O_PATH | O_DIRECTORY | O_NOFOLLOW;
sysopen(my $current, '/', $flags) or exit 0;
my $current_mount = mount_id($current);
exit 0 unless verify_mount('/', $current_mount);
my @pinned = ($current);
my $path = '';
for my $i (0 .. $#parts) {
    my $part = $parts[$i];
    $path .= "/$part";
    exit 0 unless allowed($mounts->{$path});
    exit 0 if $i >= @root && exists $mounts->{$path};
    my $parent = '/proc/self/fd/' . fileno($current);
    sysopen(my $next, "$parent/$part", $flags) or exit 0;
    my $opened_mount = mount_id($next);
    exit 0 unless defined($opened_mount);
    if ($mounts->{$path}) {
        exit 0 unless verify_mount($path, $opened_mount);
        push @pinned, $next;
    } else {
        exit 0 unless $opened_mount eq $current_mount;
    }
    $current = $next;
    $current_mount = $opened_mount;
}
my $parent = '/proc/self/fd/' . fileno($current);
opendir(my $entries, $parent) or exit 0;
while (defined(my $name = readdir($entries))) {
    next unless $name =~ /\Avzdump-[a-zA-Z0-9_.-]+\z/;
    next if exists $mounts->{"$path/$name"};
    sysopen(my $entry, "$parent/$name", O_PATH | O_NOFOLLOW) or next;
    my $entry_mount = mount_id($entry);
    next unless defined($entry_mount) && $entry_mount eq $current_mount;
    my @file = stat($entry);
    next unless @file && -f _;
    print "$file[9] $name\0";
}
