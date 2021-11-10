set -eumo pipefail

readonly node_count=3
readonly host_name=127.0.0.1
readonly tick=0.1

tmpdir=$(mktemp -d)
cd "$tmpdir"

trap cleanup EXIT QUIT
cleanup() {
	local err=$?
	local pid

	for pid in $nodes
	do
		kill -9 $pid
	done

	for pid in $nodes
	do
		set +e
		wait $pid 2>/dev/null
		local ret=$?
		set -e

		if [ $ret -ne 137 ]
		then
			err=$ret
		fi
	done

	rm -rf "$tmpdir"

	exit $err
}

fail() {
	echo $@
	exit 1
}

wait_for_port_connect() {
	local port=$1

	while ! nc -w 0 localhost $port
	do
		sleep $tick
	done
}

readonly port_base=$((RANDOM + 1024))
readonly port_top=$((port_base + 2*node_count - 1))
nodes=''
start_network() {
	[ -n "$nodes" ] && fail 'nodes already started'

	local port configs=()

	for port in $(seq $port_base 2 $port_top)
	do
		configs[$((${#configs[@]}+1))]=$(server config new $host_name:{$port,$((port+1))})
	done

	local i
	for i in $(seq $node_count)
	do
		local j node_config=${configs[i]}
		for j in $(seq $node_count)
		do
			[ $i -eq $j ] && continue
			node_config+=$'\n'$(echo "${configs[j]}" | server config get-node)
		done

		echo "$node_config" | server run &
		nodes+=" $!"
	done

	for port in $(seq $port_base $port_top)
	do
		wait_for_port_connect $port
	done
}

get_node_rpc() {
	[ -z "$nodes" ] && fail 'asking for client to stopped nodes'

	echo http://$host_name:$((port_base+1))
}

wait_for_sequence() {
	local config=$1
	local seq=$2

	until echo "$config" | client get-last-sequence | xargs test "$seq" -eq
	do
		echo "$config" | client get-last-sequence
		sleep $tick
	done
}
